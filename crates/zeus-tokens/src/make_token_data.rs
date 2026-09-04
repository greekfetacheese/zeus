use std::{collections::HashMap, io::Cursor, path::Path, path::PathBuf, str::FromStr, sync::Arc};

use alloy_primitives::{Address, FixedBytes, U256};
use anyhow::{Context, bail};
use image::codecs::png::PngEncoder;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::{error, info, warn};
use zeus_eth::{
   amm::uniswap::{
      AnyUniswapPool, DexKind, FEE_TIERS, FeeAmount, State, UniswapPool, UniswapV2Pool,
      UniswapV3Pool, UniswapV4Pool, state::batch_update_state,
   },
   currency::{Currency, ERC20Token},
   types::ChainId,
   utils::{NumericValue, address_book, batch, client::*, price_feed},
};

use crate::chains::{TokenInfo, assets_dir_for_chain};
use crate::token_data::TokenData;

/// Minimum USD value in a base currency that a pool needs to be considered liquid.
const POOL_MINIMUM_LIQUIDITY: f64 = 10_000.0;

/// Maximum number of tokens to process concurrently.
pub const DEFAULT_MAX_CONCURRENT_TOKENS: usize = 16;

impl TokenInfo {
   fn to_erc20(&self, chain_id: u64) -> ERC20Token {
      let address = Address::from_str(&self.address).unwrap();
      ERC20Token {
         chain_id,
         address,
         name: self.name.clone().into(),
         symbol: self.symbol.clone().into(),
         decimals: self.decimals,
         total_supply: U256::ZERO,
      }
   }
}

/// Step 3: liquidity-filter remaining tokens and pack icons into `TokenData`.
pub async fn make_token_data(
   work_dir: &Path,
   chains: &[ChainId],
   rpcs: &HashMap<u64, String>,
   max_concurrent: usize,
) -> anyhow::Result<Vec<TokenData>> {
   let mut all_data: Vec<TokenData> = Vec::new();

   for chain in chains {
      if chain.is_bsc() {
         info!("Skipping token encoding for BSC (same as the original util: no default BSC list)");
         continue;
      }

      let asset_dir = assets_dir_for_chain(work_dir, *chain)?;
      if !asset_dir.exists() {
         warn!("Assets directory does not exist: {:?}", asset_dir);
         continue;
      }

      let url = rpcs.get(&chain.id()).ok_or_else(|| {
         anyhow::anyhow!(
            "missing RPC URL for chain {} ({})",
            chain.id(),
            chain.name()
         )
      })?;

      let retry = retry_layer(10, 400, 330);
      let throttle = throttle_layer(30);
      let client = get_client(url, retry, throttle, 10).await?;

      let native_price = price_feed::get_eth_price(client.clone(), chain.id(), None).await?;

      let mut token_data: Vec<TokenData> = Vec::new();
      encode_chain_tokens(
         &asset_dir,
         client,
         native_price,
         *chain,
         max_concurrent,
         &mut token_data,
      )
      .await?;

      info!(
         "ChainId: {}, Tokens: {}",
         chain.id(),
         token_data.len()
      );
      all_data.extend(token_data);
   }

   Ok(all_data)
}

async fn encode_chain_tokens(
   directory: &PathBuf,
   client: RpcClient,
   native_price: f64,
   chain_id: ChainId,
   max_concurrent: usize,
   icons: &mut Vec<TokenData>,
) -> anyhow::Result<()> {
   let entries: Vec<PathBuf> = std::fs::read_dir(directory)?
      .filter_map(|entry| {
         let entry = entry.ok()?;
         let path = entry.path();
         if path.is_dir() { Some(path) } else { None }
      })
      .collect();

   let semaphore = Arc::new(Semaphore::new(max_concurrent));
   let mut join_set = JoinSet::new();

   for path in entries {
      let sem = Arc::clone(&semaphore);
      let client = client.clone();

      join_set.spawn(async move {
         let _permit = sem.acquire_owned().await.expect("semaphore closed unexpectedly");
         process_token_entry(path, client, native_price, chain_id).await
      });
   }

   let mut results: Vec<TokenData> = Vec::new();

   while let Some(res) = join_set.join_next().await {
      match res {
         Ok(Ok(Some(token_data))) => {
            results.push(token_data);
         }
         Ok(Ok(None)) => {}
         Ok(Err(e)) => {
            error!("Error processing token: {e}");
         }
         Err(e) => {
            error!("Task join error: {e}");
         }
      }
   }

   icons.extend(results);
   Ok(())
}

/// Process a single token directory concurrently.
/// Returns Some(TokenData) if it should be kept, None if skipped.
async fn process_token_entry(
   path: PathBuf,
   client: RpcClient,
   native_price: f64,
   chain_id: ChainId,
) -> anyhow::Result<Option<TokenData>> {
   if chain_id.is_bsc() {
      return Ok(None);
   }

   let logo_path = path.join("logo.png");
   let info_path = path.join("info.json");

   if !logo_path.exists() || !logo_path.is_file() {
      warn!("No logo for {:?}", path);
      return Ok(None);
   }

   if !info_path.exists() || !info_path.is_file() {
      warn!("No info.json for {:?}", path);
      return Ok(None);
   }

   let info_data = std::fs::read_to_string(&info_path)?;
   let info = serde_json::from_str::<TokenInfo>(&info_data)?;

   if Address::from_str(&info.address).is_err() {
      warn!("Invalid Ethereum address for {}", info.address);
      return Ok(None);
   }

   let token = info.to_erc20(chain_id.id());
   let keep = if token.is_base() {
      true
   } else {
      keep_token(
         client.clone(),
         native_price,
         chain_id,
         token.clone(),
      )
      .await?
   };

   if !keep {
      warn!(
         "ChainId {} - {} is not liquid",
         chain_id.id(),
         info.name
      );
      return Ok(None);
   }

   let img = match image::open(&logo_path) {
      Ok(img) => img,
      Err(e) => {
         warn!("Failed to open image for {}: {e}", info.address);
         return Ok(None);
      }
   };

   let mut write_buffer_x32 = Vec::new();
   let mut write_buffer_x24 = Vec::new();

   {
      let mut cursor = Cursor::new(&mut write_buffer_x32);
      let encoder = PngEncoder::new_with_quality(
         &mut cursor,
         image::codecs::png::CompressionType::Best,
         image::codecs::png::FilterType::Sub,
      );
      img.write_with_encoder(encoder)?;
   }

   {
      let mut cursor = Cursor::new(&mut write_buffer_x24);
      let encoder = PngEncoder::new_with_quality(
         &mut cursor,
         image::codecs::png::CompressionType::Best,
         image::codecs::png::FilterType::Sub,
      );
      img.resize(24, 24, image::imageops::FilterType::Lanczos3)
         .write_with_encoder(encoder)?;
   }

   Ok(Some(TokenData::new(
      chain_id.id(),
      info.address,
      info.name,
      info.symbol,
      info.decimals,
      write_buffer_x32,
      write_buffer_x24,
   )))
}

async fn keep_token(
   client: RpcClient,
   native_price: f64,
   chain_id: ChainId,
   token: ERC20Token,
) -> anyhow::Result<bool> {
   let concurrency = 1;
   let batch_size = 20;

   let pools = sync_pools(client.clone(), chain_id, token).await?;
   let updated_pools = batch_update_state(
      client,
      chain_id.id(),
      concurrency,
      batch_size,
      pools,
   )
   .await?;

   let mut keep_token = false;

   for pool in updated_pools {
      let base_balance = pool.base_balance();
      let base_token = pool.base_currency();

      let price = if base_token.is_native() || base_token.is_native_wrapped() {
         native_price
      } else if base_token.is_stablecoin() {
         1.0
      } else {
         0.0
      };

      let base_value = NumericValue::value(base_balance.f64(), price);

      if base_value.f64() >= POOL_MINIMUM_LIQUIDITY {
         keep_token = true;
         break;
      }
   }

   Ok(keep_token)
}

async fn sync_pools(
   client: RpcClient,
   chain_id: ChainId,
   token: ERC20Token,
) -> anyhow::Result<Vec<AnyUniswapPool>> {
   let v2_factory = address_book::uniswap_v2_factory(chain_id.id())?;
   let v3_factory = address_book::uniswap_v3_factory(chain_id.id())?;
   let state_view = address_book::uniswap_v4_stateview(chain_id.id())?;

   let mut v4_pools_map = HashMap::new();
   let mut v4_pool_ids = Vec::new();
   let mut bases_to_sync = Vec::new();

   let base_tokens = ERC20Token::base_tokens(chain_id.id());

   for base_token in &base_tokens {
      if base_token.address == token.address {
         continue;
      }

      bases_to_sync.push(base_token.clone());
      for fee in FEE_TIERS.iter() {
         let fee_amount = FeeAmount::CUSTOM(*fee);
         let pool = UniswapV4Pool::new(
            chain_id.id(),
            fee_amount,
            DexKind::UniswapV4,
            Currency::from(base_token.clone()),
            Currency::from(token.clone()),
            State::none(),
            Address::ZERO,
         );

         v4_pool_ids.push(pool.id());
         v4_pools_map.insert(pool.id(), pool);
      }
   }

   let mut tokens_map = HashMap::new();

   for base_token in &bases_to_sync {
      tokens_map.insert(base_token.address, base_token.clone());
   }

   tokens_map.insert(token.address, token.clone());

   let base_tokens_addr = bases_to_sync.iter().map(|t| t.address).collect::<Vec<_>>();
   let quote_token = token.address;

   let pools = batch::get_pools(
      client,
      chain_id.id(),
      v2_factory,
      v3_factory,
      state_view,
      v4_pool_ids,
      base_tokens_addr,
      quote_token,
   )
   .await?;

   let v2_pools = &pools.v2Pools;
   let v3_pools = &pools.v3Pools;
   let v4_pools = &pools.v4Pools;

   let mut all_v2_pools = Vec::new();
   let mut all_v3_pools = Vec::new();
   let mut all_v4_pools = Vec::new();

   for v2_pool in v2_pools {
      if v2_pool.addr.is_zero() {
         continue;
      }

      let token_a = tokens_map.get(&v2_pool.tokenA);
      let token_b = tokens_map.get(&v2_pool.tokenB);

      if token_a.is_none() {
         warn!("V2Pool Token not found: {}", v2_pool.tokenA);
         continue;
      }

      if token_b.is_none() {
         warn!("V2Pool Token not found: {}", v2_pool.tokenB);
         continue;
      }

      let token_a = token_a.unwrap();
      let token_b = token_b.unwrap();

      let pool = UniswapV2Pool::new(
         chain_id.id(),
         v2_pool.addr,
         token_a.clone(),
         token_b.clone(),
         DexKind::UniswapV2,
      );

      all_v2_pools.push(pool);
   }

   for v3_pool in v3_pools {
      if v3_pool.addr.is_zero() {
         continue;
      }

      let token_a = tokens_map.get(&v3_pool.tokenA);
      let token_b = tokens_map.get(&v3_pool.tokenB);

      if token_a.is_none() {
         warn!("V3Pool Token not found: {}", v3_pool.tokenA);
         continue;
      }

      if token_b.is_none() {
         warn!("V3Pool Token not found: {}", v3_pool.tokenB);
         continue;
      }

      let token_a = token_a.unwrap();
      let token_b = token_b.unwrap();
      let fee = v3_pool.fee.to_string().parse()?;

      let pool = UniswapV3Pool::new(
         chain_id.id(),
         v3_pool.addr,
         fee,
         token_a.clone(),
         token_b.clone(),
         DexKind::UniswapV3,
      );

      all_v3_pools.push(pool);
   }

   for v4_pool in v4_pools {
      if *v4_pool == FixedBytes::<32>::ZERO {
         continue;
      }

      let pool = v4_pools_map.get(v4_pool).context("missing v4 pool id")?;
      all_v4_pools.push(pool.clone());
   }

   let mut all_pools = Vec::new();

   for v2_pool in all_v2_pools {
      all_pools.push(v2_pool.into());
   }

   for v3_pool in all_v3_pools {
      all_pools.push(v3_pool.into());
   }

   for v4_pool in all_v4_pools {
      all_pools.push(v4_pool.into());
   }

   Ok(all_pools)
}

pub fn write_token_blob(path: &Path, tokens: &[TokenData]) -> anyhow::Result<()> {
   if let Some(parent) = path.parent() {
      if !parent.as_os_str().is_empty() {
         std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
      }
   }
   let binary_data = bincode_next::encode_to_vec(tokens, bincode_next::config::standard())?;
   std::fs::write(path, &binary_data).with_context(|| format!("write {}", path.display()))?;
   info!(
      "Wrote {} tokens ({} bytes) to {}",
      tokens.len(),
      binary_data.len(),
      path.display()
   );
   if tokens.is_empty() {
      bail!("token blob is empty — check RPCs, assets dir, and liquidity filter");
   }
   Ok(())
}
