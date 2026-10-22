use crate::persisted::{PersistedFile, client_settings_dir, ensure_data_dir, file_path};
use crate::platform::TimeStamp;
use crate::platform::fs::write_private_atomic;
use crate::sim::STORAGE_FETCH_CHUNK_SIZE;
use crate::{WalletStateKey, ZeusCtx};
use zeus_eth::{
   abi::{
      weth9,
      zeus::ZeusStateViewV3::{V3Pool, V4Pool},
   },
   alloy_primitives::{Address, U256},
   alloy_provider::Provider,
   alloy_rpc_types::BlockId,
   alloy_signer_local::PrivateKeySigner,
   alloy_sol_types::SolEvent,
   amm::uniswap::UniswapPool,
   currency::ERC20Token,
   revm::primitives::hardfork::SpecId,
   types::{ChainId, ForkConfig, SUPPORTED_CHAINS},
   utils::{batch, client::*, get_logs_for},
};

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::{
   collections::HashMap,
   sync::{Arc, Mutex, RwLock},
   time::{Duration, Instant},
};

use tokio::{sync::Semaphore, time::sleep};

/// Bound ciphertext to this logical slot (AAD).
const PROVIDER_AAD: &[u8] = b"zeus-providers-v1";

const CLIENT_SELECTION_TIMEOUT: u64 = 3;

/// Default timeout for sending a transaction or using an MEV protect rpc
pub const CLIENT_TIMEOUT_FOR_SENDING_TX: u64 = 60;

/// Default request timeout
const REQUEST_TIMEOUT: u64 = 15;

/// Default client timeout
const CLIENT_TIMEOUT: u64 = 5;

/// 8 hours in seconds
const EIGHT_HOURS: u64 = 28_800;

/// Default per-endpoint request rate (requests per second).
pub const DEFAULT_RPC_RPS: u32 = 10;

/// Default per-endpoint compute-unit budget (per second).
pub const DEFAULT_RPC_CU_PER_SECOND: u64 = 330;

/// A user-set rate must stay inside this range: 0 would stall the throttle layer and divide by
/// zero in `get_best_rpc`.
pub const MIN_RPC_RPS: u32 = 1;
pub const MAX_RPC_RPS: u32 = 1_000;
pub const MIN_RPC_CU_PER_SECOND: u64 = 1;
pub const MAX_RPC_CU_PER_SECOND: u64 = 1_000_000;

/// Websocket reconnect budget for cached clients: retry forever so a dropped
/// connection recovers on its own once the network is back.
const WS_INFINITE_RETRIES: u32 = u32::MAX;

/// Max retries
const MAX_RETRIES: u32 = 10;

/// Initial backoff
const INITIAL_BACKOFF: u64 = 400;

/// Batch size for fetching ETH balance
const ETH_BALANCE_BATCH: usize = 20;

/// Batch size for fetching ERC20 balance
const ERC20_BALANCE_BATCH: usize = 20;

/// Batch size for fetching ERC20 info
const ERC20_INFO_BATCH: usize = 20;

/// Batch size for fetching V3 pools
const VALIDATE_V4_POOLS_BATCH: usize = 20;

/// Batch size for fetching the state of V2/V3/V4 pools
const POOL_STATE_UPDATE_BATCH: usize = 20;

/// Batch size for fetching V2 pool reserves
const V2_POOL_RESERVES_BATCH: usize = 20;

/// Batch size for fetching V3 pool state
const V3_POOL_STATE_BATCH: usize = 20;

/// Batch size for fetching V4 pool state
const V4_POOL_STATE_BATCH: usize = 20;

/// Batch size for probing Multicall3 via ERC-20 allowances
const MULTICALL_BATCH: usize = 20;

/// Account count for probing JSON-RPC batch `eth_getTransactionCount`
const JSON_RPC_BATCH: usize = 20;

/// For testing only
const DEFAULT_BLOCK_RANGE: u64 = 5_000;

async fn connect_rpc(rpc: &Rpc, timeout: u64) -> Result<RpcClient, anyhow::Error> {
   RpcClientBuilder::new(rpc.url.as_ref())
      .retry(retry_layer(
         MAX_RETRIES,
         INITIAL_BACKOFF,
         rpc.cu_per_second.max(1),
      ))
      .throttle(throttle_layer(rpc.rps.max(1)))
      .timeout_secs(timeout)
      .connect()
      .await
}

/// One capability the pre-sim state fetches need from an endpoint, and that an
/// [`RpcCheck`] records.
///
/// Every one of them is on the path that builds a transaction's before-state
/// (`core::sim::diff`) or a fork's prefetch (`utils::simulate`): an endpoint that
/// cannot answer leaves the fetch empty or partial, so the diff the user reads loses
/// rows and transaction eligibility is no longer trustworthy. The top panel lists the
/// ones no endpoint can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RpcFunctionality {
   /// `eth_call` to Multicall3 — batched ERC-20 / Permit2 allowances and the NFT probes.
   Multicall,

   /// `eth_call` state overrides — batched storage-slot reads through the injected
   /// `StorageReader` bytecode. Losing it costs no rows, only speed: the slots go back
   /// to being read one at a time.
   StateOverride,

   /// JSON-RPC batch requests — batched `eth_getCode` / `eth_getTransactionCount`.
   JsonRpcBatch,

   /// The ZeusStateView contract — batched ERC-20 balances and V2/V3/V4 pool state.
   ZeusStateView,
}

impl RpcFunctionality {
   /// Every capability a check records, in the order the warning lists them.
   pub const ALL: [Self; 4] = [
      Self::Multicall,
      Self::StateOverride,
      Self::JsonRpcBatch,
      Self::ZeusStateView,
   ];

   /// What the top panel calls it.
   pub fn label(self) -> &'static str {
      match self {
         Self::Multicall => "Multicall",
         Self::StateOverride => "State Override",
         Self::JsonRpcBatch => "JSON-RPC Batch",
         Self::ZeusStateView => "Zeus StateView",
      }
   }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A check for rpc functionality
pub struct RpcCheck {
   /// True if the rpc is archive
   pub archive: bool,

   /// True if the rpc is functional at all
   ///
   /// It should at least be able to return the latest block number but it could fail on more intensive requests like `eth_getLogs`
   pub working: bool,

   /// True if the rpc is fully functional
   ///
   /// All requests should work perfect
   pub fully_functional: bool,

   /// True if the rpc can call Multicall3
   #[serde(default)]
   pub multicall: bool,

   /// True if the rpc supports `eth_call` state overrides (StorageReader)
   #[serde(default)]
   pub state_override: bool,

   /// True if the rpc supports JSON-RPC batch requests
   #[serde(default)]
   pub json_rpc_batch: bool,

   /// The block range to query for logs that the specific rpc can take it
   pub logs_block_range: u64,

   /// This is an estimation of the staticalll gas limit
   ///
   /// It means that we can make an `ethCall` that can at least use this much gas without getting an `evm timeout` error
   ///
   /// Each provider sets their own limits
   pub static_gas_limit: u64,

   /// Recommended batch size for fetching ETH balance
   pub eth_balance_batch: usize,

   /// Recommended batch size for fetching ERC20 balance
   pub erc20_balance_batch: usize,

   /// Recommended batch size for fetching ERC20 info
   pub erc20_info_batch: usize,

   /// Recommended batch size for fetching the state for V2/V3/V4 pools
   pub pool_state_update_batch: usize,

   /// Recommended batch size for fetching V3 pools
   pub validate_v4_pools_batch: usize,

   /// Recommended batch size for fetching V2 pool reserves
   pub v2_pool_reserves_batch: usize,

   /// Recommended batch size for fetching V3 pool state
   pub v3_pool_state_batch: usize,

   /// Recommended batch size for fetching V4 pool state
   pub v4_pool_state_batch: usize,

   /// Last time in UNIX timestamp we ran a check for this RPC
   pub last_check: Option<u64>,
}

impl Default for RpcCheck {
   fn default() -> Self {
      Self {
         archive: false,
         working: false,
         fully_functional: false,
         multicall: false,
         state_override: false,
         json_rpc_batch: false,
         logs_block_range: DEFAULT_BLOCK_RANGE,
         static_gas_limit: 0,
         eth_balance_batch: ETH_BALANCE_BATCH,
         erc20_balance_batch: ERC20_BALANCE_BATCH,
         erc20_info_batch: ERC20_INFO_BATCH,
         validate_v4_pools_batch: VALIDATE_V4_POOLS_BATCH,
         pool_state_update_batch: POOL_STATE_UPDATE_BATCH,
         v2_pool_reserves_batch: V2_POOL_RESERVES_BATCH,
         v3_pool_state_batch: V3_POOL_STATE_BATCH,
         v4_pool_state_batch: V4_POOL_STATE_BATCH,
         last_check: None,
      }
   }
}

impl RpcCheck {
   /// Whether this endpoint answered the probe for `functionality`.
   pub fn supports(&self, functionality: RpcFunctionality) -> bool {
      match functionality {
         RpcFunctionality::Multicall => self.multicall,
         RpcFunctionality::StateOverride => self.state_override,
         RpcFunctionality::JsonRpcBatch => self.json_rpc_batch,
         RpcFunctionality::ZeusStateView => self.stateview_working(),
      }
   }

   /// The ZeusStateView deployment answered at least one of its batch ops.
   ///
   /// All four pool checks call the same contract, so one answering is what says the
   /// contract is reachable: the probes that fail do it for their own reason — an
   /// `eth_call` the endpoint's gas cap refuses, a version the pool manager has no
   /// sample for — and would fail together with the contract itself.
   fn stateview_working(&self) -> bool {
      self.v2_pool_reserves_batch > 0
         || self.v3_pool_state_batch > 0
         || self.v4_pool_state_batch > 0
         || self.validate_v4_pools_batch > 0
   }
}

fn default_rpc_rps() -> u32 {
   DEFAULT_RPC_RPS
}

fn default_rpc_cu_per_second() -> u64 {
   DEFAULT_RPC_CU_PER_SECOND
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rpc {
   pub url: Arc<str>,
   pub chain_id: u64,
   /// False if the rpc is added by the user
   pub default: bool,
   pub enabled: bool,
   pub check: RpcCheck,
   pub mev_protect: bool,
   /// Requests per second allowed against this endpoint (throttle layer).
   #[serde(default = "default_rpc_rps")]
   pub rps: u32,
   /// Compute-units-per-second budget for the retry/backoff layer.
   #[serde(default = "default_rpc_cu_per_second")]
   pub cu_per_second: u64,
   #[serde(skip)]
   pub latency: Option<Duration>,
   /// Last time in UNIX timestamp we used this RPC
   pub last_used: u64,
   /// Last time in UNIX timestamp this RPC failed to do a request
   #[serde(default)]
   pub last_failure: Option<u64>,

   #[serde(skip)]
   pub test_in_progress: bool,
}

impl Rpc {
   #[must_use]
   pub fn builder(url: impl Into<Arc<str>>, chain_id: u64) -> RpcBuilder {
      RpcBuilder {
         url: url.into(),
         chain_id,
         default: false,
         enabled: false,
         mev_protect: false,
         rps: DEFAULT_RPC_RPS,
         cu_per_second: DEFAULT_RPC_CU_PER_SECOND,
      }
   }

   pub fn is_ws(&self) -> bool {
      self.url.starts_with("ws")
   }

   pub fn is_archive(&self) -> bool {
      self.check.archive
   }

   pub fn is_enabled(&self) -> bool {
      self.enabled
   }

   pub fn is_working(&self) -> bool {
      self.check.working
   }

   pub fn is_fully_functional(&self) -> bool {
      self.check.fully_functional
   }

   pub fn is_multicall(&self) -> bool {
      self.check.multicall
   }

   pub fn is_state_override(&self) -> bool {
      self.check.state_override
   }

   pub fn is_json_rpc_batch(&self) -> bool {
      self.check.json_rpc_batch
   }

   pub fn is_mev_protect(&self) -> bool {
      self.mev_protect
   }

   /// Clamp the rate limits into the range the transport layers can actually use.
   pub fn clamp_limits(&mut self) {
      self.rps = self.rps.clamp(MIN_RPC_RPS, MAX_RPC_RPS);
      self.cu_per_second = self.cu_per_second.clamp(MIN_RPC_CU_PER_SECOND, MAX_RPC_CU_PER_SECOND);
   }

   pub fn latency_ms(&self) -> u128 {
      self.latency.map(|latency| latency.as_millis()).unwrap_or(0)
   }

   pub fn latency_str(&self) -> String {
      if let Some(latency) = self.latency {
         format!("{}ms", latency.as_millis())
      } else {
         "N/A".to_string()
      }
   }

   pub fn should_run_check(&self) -> bool {
      let now = TimeStamp::now_as_secs().unwrap_or_default();
      if let Some(last_check) = self.check.last_check {
         let passed = now.timestamp().saturating_sub(last_check);
         passed > EIGHT_HOURS
      } else {
         true
      }
   }
}

/// Builder for [Rpc]. `url` and `chain_id` are required; flags default to false.
#[must_use = "builders do nothing unless you call build()"]
pub struct RpcBuilder {
   url: Arc<str>,
   chain_id: u64,
   default: bool,
   enabled: bool,
   mev_protect: bool,
   rps: u32,
   cu_per_second: u64,
}

impl RpcBuilder {
   /// Bundled endpoint shipped with Zeus (not added by the user).
   #[must_use]
   pub fn builtin(mut self) -> Self {
      self.default = true;
      self
   }

   #[must_use]
   pub fn enabled(mut self) -> Self {
      self.enabled = true;
      self
   }

   #[must_use]
   pub fn mev_protect(mut self) -> Self {
      self.mev_protect = true;
      self
   }

   /// Requests per second allowed against this endpoint (clamped on [`RpcBuilder::build`]).
   #[must_use]
   pub fn rps(mut self, rps: u32) -> Self {
      self.rps = rps;
      self
   }

   /// Compute-units-per-second budget (clamped on [`RpcBuilder::build`]).
   #[must_use]
   pub fn cu_per_second(mut self, cu_per_second: u64) -> Self {
      self.cu_per_second = cu_per_second;
      self
   }

   pub fn build(self) -> Rpc {
      let mut rpc = Rpc {
         url: self.url,
         chain_id: self.chain_id,
         default: self.default,
         enabled: self.enabled,
         check: RpcCheck::default(),
         mev_protect: self.mev_protect,
         rps: self.rps,
         cu_per_second: self.cu_per_second,
         latency: None,
         last_used: 0,
         last_failure: None,
         test_in_progress: false,
      };
      rpc.clamp_limits();
      rpc
   }
}

/// Map from RPC URL to RPC, keyed by `Arc<str>`.
type RpcMapByUrl = HashMap<Arc<str>, Rpc>;

fn insert_chain_rpcs(
   map: &mut HashMap<u64, RpcMapByUrl>,
   chain_id: u64,
   urls: &[&str],
   mev_urls: &[&str],
) {
   let mut rpcs = RpcMapByUrl::new();
   for url in urls {
      let url: Arc<str> = Arc::from(*url);
      rpcs.insert(
         Arc::clone(&url),
         Rpc::builder(url, chain_id).builtin().build(),
      );
   }
   for url in mev_urls {
      let url: Arc<str> = Arc::from(*url);
      rpcs.insert(
         Arc::clone(&url),
         Rpc::builder(url, chain_id).builtin().mev_protect().build(),
      );
   }
   map.insert(chain_id, rpcs);
}

/// What a cached connection is used for.
///
/// The request timeout is baked into the transport, so endpoints used with different timeout
/// budgets get separate connections rather than sharing one with the wrong timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientKind {
   /// Routine reads and queries. [`CLIENT_TIMEOUT`]
   Standard,
   /// Short targeted reads (token / NFT fetch). 10s
   Short,
   /// Transaction submission and MEV-protect endpoints. [`CLIENT_TIMEOUT_FOR_SENDING_TX`]
   Send,
}

impl ClientKind {
   pub const fn timeout_secs(self) -> u64 {
      match self {
         Self::Standard => CLIENT_TIMEOUT,
         Self::Short => 10,
         Self::Send => CLIENT_TIMEOUT_FOR_SENDING_TX,
      }
   }
}

/// Identity of a cached connection: one per endpoint per purpose.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ClientKey {
   chain: u64,
   url: Arc<str>,
   kind: ClientKind,
}

/// Lower / upper bound for the global concurrency setting.
pub const MIN_CONCURRENCY: usize = 1;
pub const MAX_CONCURRENCY: usize = 16;

fn default_concurrency() -> usize {
   2
}

/// Non-secret [`ClientManager`] settings, stored in `data/client_settings.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientSettings {
   /// Max concurrent batch/prefetch requests the client fans out. Default 1 (public-endpoint safe).
   #[serde(default = "default_concurrency")]
   pub concurrency: usize,
}

impl Default for ClientSettings {
   fn default() -> Self {
      Self {
         concurrency: default_concurrency(),
      }
   }
}

impl ClientSettings {
   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let dir = client_settings_dir()?;
      let data = std::fs::read_to_string(dir)?;
      Ok(serde_json::from_str(&data)?)
   }

   pub fn save(&self) -> Result<(), anyhow::Error> {
      ensure_data_dir()?;
      let dir = client_settings_dir()?;
      write_private_atomic(&dir, serde_json::to_string(self)?.as_bytes())?;
      Ok(())
   }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientManager {
   pub rpcs: Arc<RwLock<HashMap<u64, RpcMapByUrl>>>,

   /// Live connections, keyed by endpoint + purpose. Runtime-only, never persisted.
   #[serde(skip)]
   clients: Arc<RwLock<HashMap<ClientKey, RpcClient>>>,

   /// Runtime settings (concurrency), loaded from / saved to `data/client_settings.json`.
   #[serde(skip)]
   settings: Arc<RwLock<ClientSettings>>,

   /// What each chain's node said about the fork it runs, and when it was asked. Runtime-only.
   #[serde(skip)]
   node_forks: Arc<RwLock<HashMap<u64, NodeFork>>>,
}

/// A chain's hardfork as its node reported it (EIP-7910 `eth_config`), with the time it was asked.
///
/// `spec` is `None` when the node gave no answer Zeus can use, and that is cached too — briefly, so
/// a node without the method is not asked on every simulation, but is asked again soon in case the
/// endpoint was the problem rather than the node.
#[derive(Debug)]
struct NodeFork {
   spec: Option<SpecId>,
   /// When the question was asked, in the monotonic clock the elapsed-time TTL is measured on.
   asked_at: Instant,
   /// When the answer was read, in Unix seconds — the clock
   /// [`ChainId::spec_activation_time`] is in, so the two can be compared. `asked_at` cannot: an
   /// `Instant` has no epoch.
   asked_at_unix: u64,
}

/// How long an answer about a node's fork is reused, and how long a missing one is.
const NODE_FORK_TTL: Duration = Duration::from_secs(600);
const NODE_FORK_MISS_TTL: Duration = Duration::from_secs(30);

/// Whether a cached `eth_config` answer, read at `minted` (Unix seconds), was made before a fork
/// activation Zeus knows the time of and is therefore about the fork the chain has since left.
///
/// `now` is Unix seconds and `activation` is [`ChainId::spec_activation_time`]. A node's answer
/// describes the fork it ran when it was asked; once the chain reaches an activation Zeus has a
/// timestamp for, that answer is about the previous fork and serving it would price the simulation
/// by rules the chain has replaced. A chain Zeus has no activation time for (every L2, and the L1s
/// before Zeus ships their next fork) has no such boundary, so nothing here ages its answer — the
/// elapsed-time TTL is the only thing that does.
fn answer_predates_activation(minted: u64, now: u64, activation: Option<u64>) -> bool {
   match activation {
      Some(activation) => now >= activation && minted < activation,
      None => false,
   }
}

/// The endpoints Zeus ships, per chain: `(chain, rpc urls, mev-protect urls)`.
///
/// One table for both a fresh profile and a loaded one. It used to be a run of calls inside
/// [`ClientManager::default`], which meant a chain added in a later build only ever reached a user
/// who had no profile yet — everyone upgrading had a chain with no endpoints and no way back to
/// them.
const BUILTIN_ENDPOINTS: &[(u64, &[&str], &[&str])] = &[
   (
      1,
      &[
         "wss://ethereum-rpc.publicnode.com",
         "wss://mainnet.gateway.tenderly.co",
         "https://ethereum-rpc.publicnode.com",
         "https://eth.blockrazor.xyz",
      ],
      &[
         "https://rpc.mevblocker.io",
         "https://rpc.flashbots.net/fast",
      ],
   ),
   (
      10,
      &[
         "wss://optimism.gateway.tenderly.co",
         "wss://optimism.drpc.org",
         "wss://optimism-rpc.publicnode.com",
         "https://mainnet.optimism.io",
         "https://optimism-rpc.publicnode.com",
         "https://optimism.drpc.org",
      ],
      &[],
   ),
   (
      56,
      &[
         "wss://bsc-rpc.publicnode.com",
         "https://binance.llamarpc.com",
         "https://bsc-pokt.nodies.app",
         "https://api.zan.top/bsc-mainnet",
      ],
      &[],
   ),
   (
      8453,
      &[
         "wss://base-rpc.publicnode.com",
         "wss://base.gateway.tenderly.co",
         "https://mainnet.base.org",
         "https://1rpc.io/base",
         "https://base-rpc.publicnode.com",
      ],
      &[],
   ),
   (
      42161,
      &[
         "wss://arbitrum-one-rpc.publicnode.com",
         "https://arbitrum.meowrpc.com",
         "https://arb1.arbitrum.io/rpc",
         "https://1rpc.io/arb",
      ],
      &[],
   ),
   (
      4663,
      &[
         "https://rpc.mainnet.chain.robinhood.com/",
         "https://robinhood-rpc.publicnode.com",
         "https://robinhood.drpc.org",
      ],
      &[],
   ),
];

/// Give every chain in [`BUILTIN_ENDPOINTS`] the endpoints Zeus ships, unless `map` already mentions
/// that chain.
///
/// A chain the map *does* mention is left exactly as it is, even where it is empty: that is the
/// user's own configuration. A chain missing entirely is one this build knows and the profile
/// predates.
fn insert_missing_builtin_rpcs(map: &mut HashMap<u64, RpcMapByUrl>) {
   for &(chain, urls, mev_urls) in BUILTIN_ENDPOINTS {
      if !map.contains_key(&chain) {
         insert_chain_rpcs(map, chain, urls, mev_urls);
      }
   }
}

impl Default for ClientManager {
   fn default() -> Self {
      let mut rpc_map_by_chain = HashMap::new();
      insert_missing_builtin_rpcs(&mut rpc_map_by_chain);

      Self {
         rpcs: Arc::new(RwLock::new(rpc_map_by_chain)),
         clients: Arc::new(RwLock::new(HashMap::new())),
         settings: Arc::new(RwLock::new(ClientSettings::default())),
         node_forks: Arc::new(RwLock::new(HashMap::new())),
      }
   }
}

impl ClientManager {
   pub fn read<R>(&self, reader: impl FnOnce(&HashMap<u64, RpcMapByUrl>) -> R) -> R {
      reader(&self.rpcs.read().unwrap())
   }

   pub fn write<R>(&self, writer: impl FnOnce(&mut HashMap<u64, RpcMapByUrl>) -> R) -> R {
      writer(&mut self.rpcs.write().unwrap())
   }

   pub fn load_from_file(&self, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      let dir = Self::dir()?;
      let sealed = std::fs::read(&dir)?;
      let mut rpcs: HashMap<u64, RpcMapByUrl> = key.open_json(&sealed, PROVIDER_AAD)?;
      for rpcs_by_url in rpcs.values_mut() {
         for rpc in rpcs_by_url.values_mut() {
            rpc.clamp_limits();
         }
      }
      // A profile written by an earlier build names only the chains that existed then, so give the
      // chains it does not mention at all the endpoints Zeus ships for them.
      insert_missing_builtin_rpcs(&mut rpcs);
      self.write(|map| *map = rpcs);
      self.retain_known_clients();
      // Every chain's endpoint set was just replaced, so a fork cached from the node that used to
      // answer for it is no longer that chain's fork.
      self.forget_node_forks();
      Ok(())
   }

   /// Drop cached connections whose endpoint is no longer known (e.g. after loading a different
   /// provider set), keeping the connections for surviving endpoints warm.
   fn retain_known_clients(&self) {
      let rpcs = self.rpcs.read().unwrap();
      self
         .clients
         .write()
         .unwrap()
         .retain(|key, _| rpcs.get(&key.chain).is_some_and(|rpcs| rpcs.contains_key(&key.url)));
   }

   /// Forget the fork the node for `chain` reported.
   ///
   /// A fork schedule belongs to the node that answered: once the endpoints a chain has change, the
   /// next simulation may run on a node with a different hardfork, and reusing the old answer would
   /// price it by rules the new node does not follow.
   fn forget_node_fork(&self, chain: u64) {
      self.node_forks.write().unwrap().remove(&chain);
   }

   /// Forget every chain's node-reported fork, for when the whole endpoint set is replaced.
   pub fn forget_node_forks(&self) {
      self.node_forks.write().unwrap().clear();
   }

   pub fn save_to_file(&self, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      ensure_data_dir()?;
      let sealed = self.read(|rpcs| key.seal_json(rpcs, PROVIDER_AAD))?;
      write_private_atomic(&Self::dir()?, &sealed)?;
      Ok(())
   }

   pub fn dir() -> Result<std::path::PathBuf, anyhow::Error> {
      file_path(PersistedFile::Providers)
   }

   pub fn exists() -> Result<bool, anyhow::Error> {
      Ok(Self::dir()?.exists())
   }

   /// Max concurrent batch/prefetch requests the client fans out (see [`ClientSettings`]).
   pub fn concurrency(&self) -> usize {
      self.settings.read().unwrap().concurrency
   }

   /// Set the global concurrency, clamped to [`MIN_CONCURRENCY`]`..=`[`MAX_CONCURRENCY`].
   pub fn set_concurrency(&self, concurrency: usize) {
      self.settings.write().unwrap().concurrency =
         concurrency.clamp(MIN_CONCURRENCY, MAX_CONCURRENCY);
   }

   /// Load `client_settings.json`, keeping the defaults if it is missing or unreadable.
   pub fn load_settings(&self) {
      match ClientSettings::load_from_file() {
         Ok(mut settings) => {
            settings.concurrency = settings.concurrency.clamp(MIN_CONCURRENCY, MAX_CONCURRENCY);
            *self.settings.write().unwrap() = settings;
         }
         Err(e) => tracing::warn!("Client settings not loaded, using defaults: {e:?}"),
      }
   }

   pub fn save_settings(&self) -> Result<(), anyhow::Error> {
      let settings = self.settings.read().unwrap().clone();
      settings.save()
   }

   pub fn get_rpcs(&self, chain: u64) -> RpcMapByUrl {
      self.read(|rpcs| rpcs.get(&chain).cloned().unwrap_or_default())
   }

   pub fn add_rpc(&self, chain: u64, mut rpc: Rpc) {
      let url = rpc.url.clone();
      self.write(|rpcs| {
         let endpoints = rpcs.entry(chain).or_default();
         // The Add form can re-add a URL already listed, and it builds its record from builder
         // defaults: keep the user's own choices — MEV-protect and the custom rate limits — on
         // the replaced entry.
         if let Some(previous) = endpoints.get(&rpc.url) {
            rpc.mev_protect = previous.mev_protect;
            rpc.rps = previous.rps;
            rpc.cu_per_second = previous.cu_per_second;
         }
         endpoints.insert(rpc.url.clone(), rpc);
      });
      // Any connection cached for that URL was dialed with the replaced record's transport policy.
      self.evict_client(chain, &url);
      self.forget_node_fork(chain);
   }

   pub fn set_test_in_progress(&self, chain: u64, rpc: &Rpc, in_progress: bool) {
      self.write(|rpcs_map| {
         if let Some(rpc) = rpcs_map.get_mut(&chain).and_then(|rpcs| rpcs.get_mut(&rpc.url)) {
            rpc.test_in_progress = in_progress;
         }
      });
   }

   pub fn remove_rpc(&self, chain: u64, url: Arc<str>) {
      self.write(|rpcs| {
         rpcs.entry(chain).or_default().remove(&*url);
      });
      self.evict_client(chain, &url);
      self.forget_node_fork(chain);
   }

   fn update_rpc(&self, chain: u64, url: &str, f: impl FnOnce(&mut Rpc)) {
      self.write(|rpcs_map| {
         if let Some(rpc) = rpcs_map.get_mut(&chain).and_then(|rpcs| rpcs.get_mut(url)) {
            f(rpc);
         }
      });
   }

   /// Enable or disable an endpoint, closing its cached connections when disabled.
   pub fn set_rpc_enabled(&self, chain: u64, url: &str, enabled: bool) {
      self.update_rpc(chain, url, |rpc| rpc.enabled = enabled);
      if !enabled {
         self.evict_client(chain, url);
      }
      self.forget_node_fork(chain);
   }

   /// Update an endpoint's rate limits and drop its cached connections so the new transport
   /// layers take effect.
   pub fn set_rpc_limits(&self, chain: u64, url: &str, rps: u32, cu_per_second: u64) {
      self.update_rpc(chain, url, |rpc| {
         rpc.rps = rps.clamp(MIN_RPC_RPS, MAX_RPC_RPS);
         rpc.cu_per_second = cu_per_second.clamp(MIN_RPC_CU_PER_SECOND, MAX_RPC_CU_PER_SECOND);
      });
      self.evict_client(chain, url);
   }

   pub async fn run_latency_check_for(&self, rpc: Rpc) {
      let client = connect_rpc(&rpc, REQUEST_TIMEOUT).await;

      let client = match client {
         Ok(client) => client,
         Err(_e) => {
            #[cfg(feature = "dev")]
            tracing::error!(
               "Error connecting to client using {} {}",
               rpc.url,
               _e
            );
            return;
         }
      };

      let time = Instant::now();
      match client.get_block_number().await {
         Ok(_) => {
            let latency = time.elapsed();
            self.update_rpc(rpc.chain_id, &rpc.url, |rpc| {
               rpc.check.working = true;
               rpc.latency = Some(latency);
            });
         }
         Err(_e) => {
            #[cfg(feature = "dev")]
            tracing::error!(
               "Error latency checking for RPC: {} {}",
               rpc.url,
               _e
            );
            self.update_rpc(rpc.chain_id, &rpc.url, |rpc| {
               rpc.check.working = false;
            });
         }
      }
   }

   pub async fn run_latency_checks(&self, ctx: ZeusCtx) {
      let mut tasks = Vec::new();

      for chain in SUPPORTED_CHAINS {
         if ctx.is_chain_disabled(chain) {
            continue;
         }

         let rpcs = self.get_rpcs(chain);
         let semaphore = Arc::new(Semaphore::new(2));

         for (_url, rpc) in rpcs {
            if !rpc.is_enabled() {
               continue;
            }

            let semaphore = semaphore.clone();
            let zeus_client = self.clone();

            let task = ctx.rt().spawn(async move {
               let _permit = semaphore.acquire().await.unwrap();
               zeus_client.run_latency_check_for(rpc).await;
            });
            tasks.push(task);
         }
      }

      for task in tasks {
         let _r = task.await;
      }
   }

   pub async fn run_check_for(&self, ctx: ZeusCtx, rpc: Rpc) {
      self.set_test_in_progress(rpc.chain_id, &rpc, true);

      match rpc_test(ctx, rpc.clone()).await {
         Ok((latency, result)) => {
            self.update_rpc(rpc.chain_id, &rpc.url, |rpc| {
               rpc.check = result;
               rpc.latency = Some(latency);
            });
         }
         Err(_e) => {
            #[cfg(feature = "dev")]
            tracing::error!("Error testing RPC {} {:?}", rpc.url, _e);
            self.update_rpc(rpc.chain_id, &rpc.url, |rpc| {
               rpc.check.working = false;
            });
         }
      }

      self.set_test_in_progress(rpc.chain_id, &rpc, false);
   }

   pub async fn run_rpc_checks(&self, ctx: ZeusCtx) {
      let mut tasks = Vec::new();

      for chain in SUPPORTED_CHAINS {
         if ctx.is_chain_disabled(chain) {
            continue;
         }

         let rpcs = self.get_rpcs(chain);
         let semaphore = Arc::new(Semaphore::new(2));

         for (_url, rpc) in rpcs {
            if !rpc.is_enabled() {
               continue;
            }

            let ctx_clone = ctx.clone();
            let semaphore = semaphore.clone();
            let zeus_client = self.clone();

            let task = ctx.rt().spawn(async move {
               let _permit = semaphore.acquire().await.unwrap();
               zeus_client.run_check_for(ctx_clone, rpc).await;
            });
            tasks.push(task);
         }
      }

      for task in tasks {
         let _r = task.await;
      }
   }

   /// Mark every RPC as working
   pub fn mark_all_as_working(&self) {
      self.write(|rpcs_map| {
         for rpcs_by_url in rpcs_map.values_mut() {
            for rpc in rpcs_by_url.values_mut() {
               rpc.check.working = true;
            }
         }
      });
   }

   /// Is there any available RPC for a chain
   pub fn rpc_available(&self, chain: u64) -> bool {
      self.get_rpcs(chain).values().any(|rpc| rpc.is_enabled() && rpc.is_working())
   }

   /// Returns true if every enabled RPC that has been checked is fully functional.
   ///
   /// Unchecked endpoints are ignored so a first-run probe does not look like a malfunction.
   pub fn rpcs_fully_functional(&self, chain: u64) -> bool {
      self
         .get_rpcs(chain)
         .values()
         .filter(|rpc| rpc.is_enabled() && rpc.check.last_check.is_some())
         .all(|rpc| rpc.is_fully_functional())
   }

   /// The [`RpcFunctionality`]s no enabled, already-checked RPC on `chain` can answer.
   ///
   /// [`Self::rpcs_fully_functional`] answers "is an endpoint degraded"; this answers the
   /// follow-up the warning needs — which capability is gone *everywhere*, so the message can
   /// name what transaction eligibility loses instead of saying "something".
   ///
   /// Endpoints that were never checked are ignored, exactly like `rpcs_fully_functional`
   /// ignores them: a first-run probe must not read as a missing capability. With nothing
   /// checked the answer is empty for the same reason.
   pub fn unavailable_functionalities(&self, chain: u64) -> Vec<RpcFunctionality> {
      let checked: Vec<RpcCheck> = self
         .get_rpcs(chain)
         .into_values()
         .filter(|rpc| rpc.is_enabled() && rpc.check.last_check.is_some())
         .map(|rpc| rpc.check)
         .collect();

      if checked.is_empty() {
         return Vec::new();
      }

      RpcFunctionality::ALL
         .into_iter()
         .filter(|functionality| !checked.iter().any(|check| check.supports(*functionality)))
         .collect()
   }

   pub fn rpc_archive_available(&self, chain: u64) -> bool {
      self.get_rpcs(chain).values().any(|rpc| rpc.is_working() && rpc.is_archive())
   }

   pub fn mev_protect_available(&self, chain: u64) -> bool {
      self
         .get_rpcs(chain)
         .values()
         .any(|rpc| rpc.is_working() && rpc.is_enabled() && rpc.is_mev_protect())
   }

   /// Return the cached connection for `rpc`, dialing once on first use.
   ///
   /// The std lock is never held across the `.await`. A concurrent first-use race may dial twice; the
   /// first insert wins and the extra socket is dropped when the losing client is returned by value.
   async fn cached_connect(&self, rpc: &Rpc, kind: ClientKind) -> Result<RpcClient, anyhow::Error> {
      let key = ClientKey {
         chain: rpc.chain_id,
         url: rpc.url.clone(),
         kind,
      };

      if let Some(client) = self.clients.read().unwrap().get(&key).cloned() {
         return Ok(client);
      }

      let client = RpcClientBuilder::new(rpc.url.as_ref())
         .retry(retry_layer(
            MAX_RETRIES,
            INITIAL_BACKOFF,
            rpc.cu_per_second.max(1),
         ))
         .throttle(throttle_layer(rpc.rps.max(1)))
         .timeout_secs(kind.timeout_secs())
         .ws_max_retries(WS_INFINITE_RETRIES)
         .connect()
         .await?;

      let mut cache = self.clients.write().unwrap();
      Ok(cache.entry(key).or_insert(client).clone())
   }

   /// A cached connection for `rpc` under the given [`ClientKind`] timeout policy.
   pub async fn client_for(&self, rpc: &Rpc, kind: ClientKind) -> Result<RpcClient, anyhow::Error> {
      self.cached_connect(rpc, kind).await
   }

   /// Drop every cached connection for one endpoint (all purposes).
   pub fn evict_client(&self, chain: u64, url: &str) {
      self
         .clients
         .write()
         .unwrap()
         .retain(|key, _| !(key.chain == chain && &*key.url == url));
   }

   /// A cached [`ClientKind::Standard`] connection for `rpc`.
   pub async fn connect_to(&self, rpc: &Rpc) -> Result<RpcClient, anyhow::Error> {
      self.cached_connect(rpc, ClientKind::Standard).await
   }

   pub async fn get_client(&self, chain: u64) -> Result<RpcClient, anyhow::Error> {
      let time_passed = Instant::now();
      let timeout = Duration::from_secs(CLIENT_SELECTION_TIMEOUT);

      loop {
         if time_passed.elapsed() > timeout {
            return Err(anyhow!(
               "Failed to get client for chain {} Timeout exceeded",
               chain
            ));
         }

         if let Some(rpc) = self.get_best_rpc(chain) {
            let c = match self.connect_to(&rpc).await {
               Ok(client) => client,
               Err(_e) => {
                  #[cfg(feature = "dev")]
                  tracing::error!(
                     "Error connecting to client using {} for chain {}: {:?}",
                     rpc.url,
                     chain,
                     _e
                  );
                  self.penalize(chain, &rpc);
                  sleep(Duration::from_millis(100)).await;
                  continue;
               }
            };
            return Ok(c);
         } else {
            sleep(Duration::from_millis(100)).await;
         }
      }
   }

   pub async fn get_mev_protect_client(&self, chain: u64) -> Result<RpcClient, anyhow::Error> {
      let time_passed = Instant::now();
      let timeout = Duration::from_secs(CLIENT_SELECTION_TIMEOUT);

      while !self.mev_protect_available(chain) {
         if time_passed.elapsed() > timeout {
            return Err(anyhow!(
               "Failed to get MEV protect client for chain {} Timeout exceeded",
               chain
            ));
         }
         sleep(Duration::from_millis(100)).await;
      }

      let rpcs = self.get_rpcs(chain);

      for rpc in rpcs.values() {
         if !rpc.mev_protect || !rpc.is_working() || !rpc.is_enabled() {
            continue;
         }

         match self.client_for(rpc, ClientKind::Send).await {
            Ok(client) => return Ok(client),
            Err(_e) => {
               #[cfg(feature = "dev")]
               tracing::error!(
                  "Error connecting to client using {} for chain {}: {:?}",
                  rpc.url,
                  chain,
                  _e
               );
            }
         }
      }

      Err(anyhow!(
         "No MEV protect clients found for chain {}",
         chain
      ))
   }

   pub async fn get_archive_client(
      &self,
      chain: u64,
      http: bool,
   ) -> Result<RpcClient, anyhow::Error> {
      let time_passed = Instant::now();
      let timeout = Duration::from_secs(CLIENT_SELECTION_TIMEOUT);

      while !self.rpc_archive_available(chain) {
         if time_passed.elapsed() > timeout {
            return Err(anyhow!(
               "Failed to get archive client for chain {} Timeout exceeded",
               chain
            ));
         }
         sleep(Duration::from_millis(100)).await;
      }

      let rpcs = self.get_rpcs(chain);

      for rpc in archive_candidates(rpcs.values().cloned(), http) {
         match self.connect_to(&rpc).await {
            Ok(client) => return Ok(client),
            Err(_e) => {
               #[cfg(feature = "dev")]
               tracing::error!(
                  "Error connecting to client using {} for chain {}: {:?}",
                  rpc.url,
                  chain,
                  _e
               );
            }
         }
      }

      Err(anyhow!(
         "No archive clients found for chain {}",
         chain
      ))
   }

   fn penalize(&self, chain: u64, rpc: &Rpc) {
      let now = TimeStamp::now_as_millis().unwrap_or_default();
      self.update_rpc(chain, &rpc.url, |rpc| {
         rpc.last_failure = Some(now.timestamp());
      });
   }

   /// Select the best RPC for the given chain
   ///
   /// A **fully-functional** endpoint always wins over a partial one. The score — latency, plus
   /// how recently this endpoint was used (spreads concurrent load), plus a decaying penalty for
   /// its last failure — only ranks endpoints *inside* one class, so load balancing never moves
   /// work onto an endpoint that fails the capability checks while a fully-functional one is
   /// enabled and working. The partials are consulted only once none of those is left, and then
   /// the same score picks among them.
   pub fn get_best_rpc(&self, chain: u64) -> Option<Rpc> {
      let failure_penalty_max: u128 = 10_000;
      let failure_decay_secs: u64 = 60;

      self.write(|rpcs_map| {
         let Some(rpcs) = rpcs_map.get_mut(&chain) else {
            return None;
         };
         let now_ms = TimeStamp::now_as_millis().unwrap_or_default().timestamp();
         let mut best_key = None;
         let mut best_fully = false;
         let mut best_score = u128::MAX;

         for (url, rpc) in rpcs.iter_mut() {
            if !rpc.is_enabled() || !rpc.is_working() {
               continue;
            }

            let cooldown_ms: u64 = 1000 / u64::from(rpc.rps.max(1));
            let time_since_used = now_ms.saturating_sub(rpc.last_used);
            let usage_penalty = cooldown_ms.saturating_sub(time_since_used) as u128;

            let mut score = rpc.latency_ms() + usage_penalty;

            if let Some(lf) = rpc.last_failure {
               let time_since_fail = now_ms.saturating_sub(lf);
               if time_since_fail < failure_decay_secs * 1000 {
                  let remaining = (failure_decay_secs * 1000 - time_since_fail) as u128;
                  let fail_penalty =
                     failure_penalty_max * remaining / (failure_decay_secs as u128 * 1000);
                  score += fail_penalty;
               } else {
                  rpc.last_failure = None;
               }
            }

            let fully = rpc.is_fully_functional();

            // Class first, score second: a partial endpoint never displaces a fully-functional
            // one, however much cheaper its score is.
            let better = match best_key {
               None => true,
               Some(_) => match (fully, best_fully) {
                  (true, false) => true,
                  (false, true) => false,
                  _ => score < best_score,
               },
            };

            if better {
               best_score = score;
               best_fully = fully;
               best_key = Some(url.clone());
            }
         }

         let Some(key) = best_key else {
            return None;
         };

         rpcs.get_mut(&key).map(|rpc| {
            rpc.last_used = now_ms;
            rpc.clone()
         })
      })
   }

   /// Execute a request with automatic RPC selection, retries, and load balancing.
   ///
   /// `f` receives a connected Provider (RpcClient) and returns a future with the result. It is called
   /// once per retry attempt, so it must be reusable (an `Fn` closure returning a future).
   /// Retries across RPCs on failure, up to MAX_RETRIES total attempts.
   /// Selects RPC based on latency + usage cooldown to spread concurrent load.
   pub async fn request<F, Fut, R>(&self, chain: u64, f: F) -> Result<R, anyhow::Error>
   where
      F: Fn(RpcClient) -> Fut,
      Fut: core::future::Future<Output = Result<R, anyhow::Error>>,
   {
      self.request_with(chain, (), |client, _| f(client)).await
   }

   /// The hardfork a chain's own node says it runs, for the local simulation.
   ///
   /// Zeus's per-chain spec is a build-time guess about somebody else's node, so the node is asked
   /// what it is configured for (EIP-7910 `eth_config`) and its answer wins:
   /// [`ChainId::spec_from_node`] turns it into a spec. Whatever the node cannot answer — the
   /// method is not implemented, the chain is one Zeus does not track, the fork is not one Zeus can
   /// place — comes back as `None` and leaves the caller on [`ChainId::spec`].
   ///
   /// Cached per chain: an answer Zeus can use is kept for [`NODE_FORK_TTL`], a missing one for
   /// [`NODE_FORK_MISS_TTL`] — but neither outlives a fork activation Zeus knows the time of. Once
   /// the wall clock passes that activation, an answer read before it is dropped and the node is
   /// asked again, so a tx simulated across the boundary is not priced by the rules the chain just
   /// replaced (see [`answer_predates_activation`]).
   pub async fn node_spec(&self, chain: u64) -> Option<SpecId> {
      let chain_id = ChainId::new(chain).ok()?;

      // `eth_config` is an L1 method: the L2s do not implement it, so asking would spend a request
      // and its retries on every simulation to be told nothing. Only the chains whose fork schedule
      // Zeus tracks by hand have anything to ask about — the same gate `spec_from_node` applies,
      // moved ahead of the request — and they are left on the spec Zeus ships.
      if chain_id.spec_activation_time().is_none() {
         return None;
      }

      let cached = self
         .node_forks
         .read()
         .unwrap()
         .get(&chain)
         .map(|fork| (fork.spec, fork.asked_at, fork.asked_at_unix));
      if let Some((spec, asked_at, asked_at_unix)) = cached {
         let ttl = if spec.is_some() {
            NODE_FORK_TTL
         } else {
            NODE_FORK_MISS_TTL
         };
         // A missing answer is still an answer about the pre-fork node, so the activation boundary
         // applies to it too, ahead of its shorter TTL.
         let now = TimeStamp::now_as_secs().unwrap_or_default().timestamp();
         let elapsed_within_ttl = asked_at.elapsed() < ttl;
         if elapsed_within_ttl
            && !answer_predates_activation(
               asked_at_unix,
               now,
               chain_id.spec_activation_time(),
            )
         {
            return spec;
         }
      }

      // `eth_config` answers with the fork's configuration object under `current`; a node that does
      // not implement the method fails the request, which is the same as saying nothing.
      let config = self
         .request(chain, |client| async move {
            let result: serde_json::Value = client
               .raw_request("eth_config".into(), ())
               .await
               .map_err(|e| anyhow!("{:?}", e))?;
            Ok(result)
         })
         .await
         .ok()
         .and_then(|result| {
            // The fork's configuration object; anything else in the response is not needed.
            let current = result.get("current")?.clone();
            serde_json::from_value::<ForkConfig>(current).ok()
         });

      let spec = config.as_ref().and_then(|conf| chain_id.spec_from_node(conf));

      // The node is the authority on the fork it runs, so a disagreement with what Zeus was built
      // with is worth saying out loud: without this the simulation is silently priced by rules the
      // chain may have replaced, which is how a Sepolia transfer came to estimate 21,000 against
      // the 204,600 the chain charges.
      if let Some(conf) = &config {
         let known = chain_id.spec_activation_time();
         if conf.activation_time.is_some() && conf.activation_time != known {
            tracing::warn!(
               "{chain_id:?} reports a fork activated at {:?}, Zeus knows {:?} — simulating on {:?}",
               conf.activation_time,
               known,
               spec.unwrap_or_else(|| chain_id.spec())
            );
         }
      }

      self.node_forks.write().unwrap().insert(
         chain,
         NodeFork {
            spec,
            asked_at: Instant::now(),
            asked_at_unix: TimeStamp::now_as_secs().unwrap_or_default().timestamp(),
         },
      );

      spec
   }

   /// The spec from a cached `eth_config` answer, without touching the network.
   ///
   /// The send screen's Max amount is sized on the frame path, where the async [`Self::node_spec`]
   /// cannot run, so the reserve it computes reads the node's answer the same way the simulation
   /// will: from this cache. `None` when nothing was cached, so the caller falls back to
   /// [`ChainId::spec`] — exactly what a simulation with no node answer does.
   ///
   /// Deliberately does **not** re-apply [`Self::node_spec`]'s TTL or activation boundary: a stale
   /// answer is still the node's own word about a fork Zeus can name, and serving it keeps Max on
   /// the newer (possibly more expensive) spec rather than dropping to the shipped one — the
   /// cheaper-hardfork shortfall this exists to prevent. The async path refreshes the entry; the
   /// frame path cannot, and reading the freshest cached answer it has is strictly safer than
   /// ignoring it.
   pub fn cached_node_spec(&self, chain: u64) -> Option<SpecId> {
      self.node_forks.read().unwrap().get(&chain).and_then(|fork| fork.spec)
   }

   /// Like [`Self::request`], but takes an owned, clonable payload that is handed to `f` on every
   /// attempt. This removes the `|client| { let x = x.clone(); async move { … } }` boilerplate: pass
   /// the captured value once, and this method clones it per retry while you return the future
   /// directly instead of wrapping it in an `async move` block:
   ///
   /// ```ignore
   /// client
   ///    .request_with(chain, tokens_addr, |client, tokens_addr| {
   ///       batch::get_erc20_balances(client, chain, None, owner, tokens_addr)
   ///    })
   ///    .await?;
   /// ```
   ///
   /// Pass a tuple for more than one value. Use [`Self::request`] when nothing needs owning.
   pub async fn request_with<A, F, Fut, R>(
      &self,
      chain: u64,
      args: A,
      f: F,
   ) -> Result<R, anyhow::Error>
   where
      A: Clone,
      F: Fn(RpcClient, A) -> Fut,
      Fut: core::future::Future<Output = Result<R, anyhow::Error>>,
   {
      let mut attempts = 0;
      let start = Instant::now();

      while attempts < MAX_RETRIES as usize {
         let rpc = self.get_best_rpc(chain);

         let rpc = match rpc {
            Some(rpc) => rpc,
            None => {
               attempts += 1;
               sleep(Duration::from_millis(INITIAL_BACKOFF)).await;
               continue;
            }
         };

         let client = match self.connect_to(&rpc).await {
            Ok(client) => client,
            Err(_e) => {
               #[cfg(feature = "dev")]
               tracing::warn!("Failed to connect to {}: {:?}", rpc.url, _e);
               // Do not mark it as not working, could be a network issue
               attempts += 1;
               self.penalize(chain, &rpc);
               continue;
            }
         };

         match f(client, args.clone()).await {
            Ok(res) => return Ok(res),
            Err(_e) => {
               self.penalize(chain, &rpc);
               #[cfg(feature = "dev")]
               tracing::warn!("Request failed on {}: {:?}", rpc.url, _e);
               attempts += 1;
               sleep(Duration::from_millis(INITIAL_BACKOFF)).await;
            }
         }

         if start.elapsed() > Duration::from_secs(REQUEST_TIMEOUT) {
            return Err(anyhow!("Request timed out for chain {}", chain));
         }
      }

      Err(anyhow!("Exhausted retries for chain {}", chain))
   }
}

/// Archive endpoints to dial, fully-functional ones first.
///
/// The sort is stable, so the caller's order (the endpoint map's) breaks ties inside a class. A
/// historical read landing on an endpoint that fails the capability checks is what the ordering
/// exists to avoid: the Railgun provider asks this client for old logs, and a partial endpoint
/// answers with a smaller block range, or not at all.
fn archive_candidates(rpcs: impl IntoIterator<Item = Rpc>, http: bool) -> Vec<Rpc> {
   let mut candidates: Vec<Rpc> = rpcs
      .into_iter()
      .filter(|rpc| rpc.is_enabled() && rpc.is_working() && rpc.is_archive())
      .filter(|rpc| !(http && rpc.is_ws()))
      .collect();

   candidates.sort_by_key(|rpc| !rpc.is_fully_functional());
   candidates
}

/// Try to determine if the given RPC is working
///
/// Eg. Some free endpoints don't support `eth_getLogs` in the free tier
///
/// Others have a very low staticalll gas limit which cause the batch requests to fail
async fn rpc_test(ctx: ZeusCtx, rpc: Rpc) -> Result<(Duration, RpcCheck), anyhow::Error> {
   #[cfg(feature = "dev")]
   tracing::debug!("Testing {}", rpc.url);

   let client = connect_rpc(&rpc, CLIENT_TIMEOUT_FOR_SENDING_TX).await?;
   let chain = rpc.chain_id;

   let time = Instant::now();
   let latest_block = client.get_block_number().await?;
   let latency = time.elapsed();

   let result = Arc::new(Mutex::new(RpcCheck::default()));

   // If it can return at least the latest block is considered functional
   {
      let mut guard = result.lock().unwrap();
      guard.working = true;
   }

   // Deep enough that a pruned full node's ~128-block state window is far behind; a young chain
   // simply probes from its earliest block instead of failing the whole check.
   let block_to_query = latest_block.saturating_sub(100_000);

   let weth = ERC20Token::wrapped_native_token(rpc.chain_id);

   archive_check(
      client.clone(),
      block_to_query,
      weth.address,
      result.clone(),
   )
   .await;

   get_logs_check(
      client.clone(),
      weth.address,
      latest_block,
      result.clone(),
   )
   .await;

   multicall_check(client.clone(), weth.address, result.clone()).await;
   state_override_check(client.clone(), weth.address, result.clone()).await;
   json_rpc_batch_check(client.clone(), result.clone()).await;
   v2_pool_reserves_check(ctx.clone(), client.clone(), chain, result.clone()).await;
   v3_pool_state_check(ctx.clone(), client.clone(), chain, result.clone()).await;
   v4_pool_state_check(ctx.clone(), client.clone(), chain, result.clone()).await;
   validate_v4_pools_check(ctx.clone(), client.clone(), chain, result.clone()).await;

   {
      let now = TimeStamp::now_as_secs()?;
      let mut guard = result.lock().unwrap();
      guard.last_check = Some(now.timestamp());
      // V3 state batch calls are the most expensive in terms of gas
      // So will use this as a reference for the pool state update batch
      guard.pool_state_update_batch = guard.v3_pool_state_batch;
      guard.fully_functional = guard.logs_block_range > 0
         && guard.v2_pool_reserves_batch > 0
         && guard.v3_pool_state_batch > 0
         && guard.v4_pool_state_batch > 0
         && guard.validate_v4_pools_batch > 0
         && guard.multicall
         && guard.state_override
         && guard.json_rpc_batch;
   }

   let result = result.lock().unwrap().clone();

   #[cfg(feature = "dev")]
   tracing::debug!(
      "Tested {} in {}secs",
      rpc.url,
      time.elapsed().as_secs_f32()
   );

   Ok((latency, result))
}

async fn multicall_check(client: RpcClient, weth: Address, result: Arc<Mutex<RpcCheck>>) {
   let owner = Address::ZERO;
   let pairs: Vec<(Address, Address)> = (1..=MULTICALL_BATCH)
      .map(|_i| {
         let signer = PrivateKeySigner::random();
         (weth, signer.address())
      })
      .collect();

   let ok = match batch::get_erc20_allowances(client, owner, pairs, None).await {
      Ok(rows) => rows.len() == MULTICALL_BATCH,
      Err(_e) => {
         #[cfg(feature = "dev")]
         tracing::debug!("Multicall Check Error: {:?}", _e);
         false
      }
   };

   let mut guard = result.lock().unwrap();
   guard.multicall = ok;
}

async fn state_override_check(client: RpcClient, account: Address, result: Arc<Mutex<RpcCheck>>) {
   let slots: Vec<U256> = (0..STORAGE_FETCH_CHUNK_SIZE).map(U256::from).collect();

   let ok = match batch::get_account_storage(client, account, slots, None).await {
      Ok(read) => read.values.len() == STORAGE_FETCH_CHUNK_SIZE,
      Err(_e) => {
         #[cfg(feature = "dev")]
         tracing::debug!("State Override Check Error: {:?}", _e);
         false
      }
   };

   let mut guard = result.lock().unwrap();
   guard.state_override = ok;
}

async fn json_rpc_batch_check(client: RpcClient, result: Arc<Mutex<RpcCheck>>) {
   let accounts: Vec<Address> =
      (0..JSON_RPC_BATCH).map(|_| PrivateKeySigner::random().address()).collect();

   let ok = match batch::get_account_nonces(client, accounts, None).await {
      Ok(nonces) => nonces.len() == JSON_RPC_BATCH,
      Err(_e) => {
         #[cfg(feature = "dev")]
         tracing::debug!("JSON-RPC Batch Check Error: {:?}", _e);
         false
      }
   };

   let mut guard = result.lock().unwrap();
   guard.json_rpc_batch = ok;
}

/// Archive means "can serve historical **state**", not "can serve an old block".
///
/// A pruned full node still answers `eth_getBlockByNumber` for old heights — geth keeps them in its
/// freezer — while what it actually drops is the state trie behind them. So the probe is a balance
/// read: a pruned node errors ("missing trie node" / "historical state is not available"), an
/// archive node answers, and that answer is legitimately allowed to be zero, which is why this tests
/// `is_ok` rather than the value.
async fn archive_check(
   client: RpcClient,
   block_to_query: u64,
   probe: Address,
   result: Arc<Mutex<RpcCheck>>,
) {
   let old_balance = client.get_balance(probe).block_id(BlockId::number(block_to_query)).await;

   let is_archive = old_balance.is_ok();

   let mut guard = result.lock().unwrap();
   guard.archive = is_archive;
}

async fn get_logs_check(
   client: RpcClient,
   weth_address: Address,
   latest_block: u64,
   result: Arc<Mutex<RpcCheck>>,
) {
   // Weth deposit is a very frequent event, we only query 10 blocks back
   let block_range = 10;
   let from_block = latest_block.saturating_sub(block_range);
   let mut success = false;

   let res = get_logs_for(
      client,
      vec![weth_address],
      vec![weth9::Deposit::SIGNATURE],
      from_block,
      1,
      block_range,
   )
   .await;

   match res {
      Ok(_) => success = true,
      Err(_e) => {
         #[cfg(feature = "dev")]
         tracing::debug!("eth_getLogs Check Error: {:?}", _e);
      }
   }

   let mut guard = result.lock().unwrap();
   if success {
      guard.logs_block_range = DEFAULT_BLOCK_RANGE;
   } else {
      guard.logs_block_range = 0;
   }
}

async fn v2_pool_reserves_check(
   ctx: ZeusCtx,
   client: RpcClient,
   chain: u64,
   result: Arc<Mutex<RpcCheck>>,
) {
   let sample: Vec<_> = ctx
      .pool_manager()
      .get_v2_pools_for_chain(chain)
      .into_iter()
      .take(V2_POOL_RESERVES_BATCH)
      .collect();

   let mut success = false;

   let client = client.clone();
   let pools: Vec<_> =
      sample.iter().take(V2_POOL_RESERVES_BATCH).map(|pool| pool.address()).collect();

   match batch::get_v2_reserves(client, chain, pools).await {
      Ok(_) => success = true,
      Err(_e) => {
         #[cfg(feature = "dev")]
         tracing::debug!("V2 Reserves Check Error: {:?}", _e);
      }
   }

   let mut guard = result.lock().unwrap();
   guard.v2_pool_reserves_batch = if success { V2_POOL_RESERVES_BATCH } else { 0 };
}

async fn v3_pool_state_check(
   ctx: ZeusCtx,
   client: RpcClient,
   chain: u64,
   result: Arc<Mutex<RpcCheck>>,
) {
   let sample: Vec<_> = ctx
      .pool_manager()
      .get_v3_pools_for_chain(chain)
      .into_iter()
      .take(V3_POOL_STATE_BATCH)
      .collect();

   let mut success = false;

   let client = client.clone();
   let pools: Vec<_> = sample
      .iter()
      .take(V3_POOL_STATE_BATCH)
      .map(|pool| V3Pool {
         addr: pool.address(),
         tokenA: pool.currency0().address(),
         tokenB: pool.currency1().address(),
         fee: pool.fee().fee_u24(),
      })
      .collect();

   match batch::get_v3_state(client, chain, pools).await {
      Ok(_) => success = true,
      Err(_e) => {
         #[cfg(feature = "dev")]
         tracing::debug!("V3 State Check Error: {:?}", _e);
      }
   }

   let mut guard = result.lock().unwrap();
   guard.v3_pool_state_batch = if success { V3_POOL_STATE_BATCH } else { 0 };
}

async fn v4_pool_state_check(
   ctx: ZeusCtx,
   client: RpcClient,
   chain: u64,
   result: Arc<Mutex<RpcCheck>>,
) {
   let sample: Vec<_> = ctx
      .pool_manager()
      .get_v4_pools_for_chain(chain)
      .into_iter()
      .take(V4_POOL_STATE_BATCH)
      .collect();

   let mut success = false;

   let client = client.clone();
   let pools: Vec<_> = sample
      .iter()
      .take(V4_POOL_STATE_BATCH)
      .map(|pool| V4Pool {
         pool: pool.id(),
         tickSpacing: pool.tick_spacing(),
      })
      .collect();

   match batch::get_v4_pool_state(client, chain, pools).await {
      Ok(_) => success = true,
      Err(_e) => {
         #[cfg(feature = "dev")]
         tracing::debug!("V4 State Check Error: {:?}", _e);
      }
   }

   let mut guard = result.lock().unwrap();
   guard.v4_pool_state_batch = if success { V4_POOL_STATE_BATCH } else { 0 };
}

async fn validate_v4_pools_check(
   ctx: ZeusCtx,
   client: RpcClient,
   chain: u64,
   result: Arc<Mutex<RpcCheck>>,
) {
   let sample: Vec<_> = ctx
      .pool_manager()
      .get_v4_pools_for_chain(chain)
      .into_iter()
      .take(VALIDATE_V4_POOLS_BATCH)
      .collect();

   let mut success = false;

   let client = client.clone();
   let pools: Vec<_> = sample.iter().take(VALIDATE_V4_POOLS_BATCH).map(|pool| pool.id()).collect();

   match batch::validate_v4_pools(client, chain, pools).await {
      Ok(_) => success = true,
      Err(_e) => {
         #[cfg(feature = "dev")]
         tracing::debug!("V4 Validate Pools Check Error: {:?}", _e);
      }
   }

   let mut guard = result.lock().unwrap();
   guard.validate_v4_pools_batch = if success { VALIDATE_V4_POOLS_BATCH } else { 0 };
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::alloy_provider::Provider;

   #[tokio::test]
   async fn test_rpcs() {
      let zeus_client = ClientManager::default();
      zeus_client.mark_all_as_working();

      let chain = 1;
      let time = Instant::now();
      let mut tasks = Vec::new();

      for _ in 0..30 {
         let zeus_client = zeus_client.clone();

         tasks.push(
            crate::platform::runtime::test_handle().spawn(async move {
               let res = zeus_client
                  .request(chain, |client| async move {
                     let block = client.get_block_number().await?;
                     Ok(block)
                  })
                  .await;
               match res {
                  Ok(_block) => {}
                  Err(e) => {
                     eprintln!("error: {:?}", e);
                  }
               }
            }),
         );
      }

      for task in tasks {
         task.await.unwrap();
      }

      let elapsed = time.elapsed();
      println!("Time: {}secs", elapsed.as_secs_f32());
   }

   /// Requires the public endpoints below to answer. Proves the whole path against real nodes: the
   /// request, the parse, and the resolution — mainnet reports Osaka's activation time, Sepolia
   /// Amsterdam's, and an L2 gets nothing out of its own answer.
   ///
   /// The endpoints are added rather than taken from [`ClientManager::default`], whose list is
   /// disabled until the app probes it.
   #[tokio::test]
   #[ignore = "needs working public endpoints"]
   async fn node_spec_against_live_nodes() {
      let zeus_client = ClientManager::default();

      for (chain, urls) in [
         (
            1u64,
            [
               "https://ethereum-rpc.publicnode.com",
               "https://eth.blockrazor.xyz",
            ],
         ),
         (
            11155111,
            [
               "https://ethereum-sepolia-rpc.publicnode.com",
               "https://sepolia.drpc.org",
            ],
         ),
         (
            8453,
            [
               "https://base-rpc.publicnode.com",
               "https://mainnet.base.org",
            ],
         ),
      ] {
         for url in urls {
            let mut rpc = Rpc::builder(url, chain).enabled().build();
            rpc.check.working = true;
            rpc.check.fully_functional = true;
            zeus_client.add_rpc(chain, rpc);
         }
      }

      assert_eq!(
         zeus_client.node_spec(1).await,
         Some(SpecId::OSAKA)
      );
      assert_eq!(
         zeus_client.node_spec(11155111).await,
         Some(SpecId::AMSTERDAM)
      );

      // An L2 is left on the spec Zeus ships without being asked at all: `eth_config` is an L1
      // method, so the request would cost a round trip and its retries on every simulation to be
      // told nothing. Nothing is cached for it either — which is how this asserts the request was
      // never attempted, endpoints or no endpoints.
      assert_eq!(zeus_client.node_spec(8453).await, None);
      assert!(!zeus_client.node_forks.read().unwrap().contains_key(&8453));
   }

   #[test]
   fn test_seal_open_roundtrip() {
      let key = WalletStateKey::generate().unwrap();
      let client = ClientManager::default();
      let sealed = client.read(|rpcs| key.seal_json(rpcs, PROVIDER_AAD)).unwrap();
      let loaded: HashMap<u64, RpcMapByUrl> = key.open_json(&sealed, PROVIDER_AAD).unwrap();
      assert!(!loaded.is_empty());
      assert!(key.open_json::<HashMap<u64, RpcMapByUrl>>(&sealed, b"wrong-aad").is_err());
   }

   fn rpc_for_select(url: &str, fully: bool, latency_ms: u64) -> Rpc {
      let mut rpc = Rpc::builder(url, 1).enabled().build();
      rpc.check.working = true;
      rpc.check.fully_functional = fully;
      rpc.latency = Some(Duration::from_millis(latency_ms));
      rpc
   }

   fn client_with(rpcs: impl IntoIterator<Item = Rpc>) -> ClientManager {
      let client = ClientManager {
         rpcs: Arc::new(RwLock::new(HashMap::new())),
         clients: Arc::new(RwLock::new(HashMap::new())),
         settings: Arc::new(RwLock::new(ClientSettings::default())),
         node_forks: Arc::new(RwLock::new(HashMap::new())),
      };
      for rpc in rpcs {
         client.add_rpc(rpc.chain_id, rpc);
      }
      client
   }

   /// Enabled + working http endpoint; building a client performs no network I/O.
   fn http_rpc(url: &str) -> Rpc {
      let mut rpc = Rpc::builder(url, 1).enabled().build();
      rpc.check.working = true;
      rpc
   }

   #[tokio::test]
   async fn disabling_an_rpc_evicts_its_connection() {
      let client = client_with([http_rpc("http://127.0.0.1:1")]);
      let rpc = client.get_best_rpc(1).unwrap();
      let _ = client.client_for(&rpc, ClientKind::Standard).await.unwrap();
      assert_eq!(client.clients.read().unwrap().len(), 1);

      client.set_rpc_enabled(1, "http://127.0.0.1:1", false);
      assert!(client.clients.read().unwrap().is_empty());
      assert!(!client.get_rpcs(1).values().any(|rpc| rpc.enabled));
   }

   /// A re-add of a listed URL replaces its record, so a connection cached for it was dialed with
   /// the replaced record's transport policy and must go.
   #[tokio::test]
   async fn add_rpc_evicts_the_connection_of_the_url_it_replaces() {
      let client = client_with([http_rpc("http://127.0.0.1:1")]);
      let rpc = client.get_best_rpc(1).unwrap();
      let _ = client.client_for(&rpc, ClientKind::Standard).await.unwrap();
      assert_eq!(client.clients.read().unwrap().len(), 1);

      client.add_rpc(1, http_rpc("http://127.0.0.1:1"));
      assert!(client.clients.read().unwrap().is_empty());
   }

   /// The Add form builds a fresh record from builder defaults, so re-adding a URL already listed
   /// has to keep the user's own choices on the entry.
   #[test]
   fn add_rpc_keeps_the_users_choices_when_replacing_a_listed_url() {
      let client = client_with([http_rpc("http://127.0.0.1:1")]);
      client.update_rpc(1, "http://127.0.0.1:1", |rpc| {
         rpc.mev_protect = true;
         rpc.rps = 7;
         rpc.cu_per_second = 4242;
      });

      client.add_rpc(
         1,
         Rpc::builder("http://127.0.0.1:1", 1).enabled().build(),
      );

      let rpcs = client.get_rpcs(1);
      let rpc = rpcs.get("http://127.0.0.1:1").unwrap();
      assert!(rpc.mev_protect);
      assert_eq!(rpc.rps, 7);
      assert_eq!(rpc.cu_per_second, 4242);
   }

   #[tokio::test]
   async fn retain_known_clients_drops_removed_endpoints() {
      let client = client_with([
         http_rpc("http://127.0.0.1:1"),
         http_rpc("http://127.0.0.1:2"),
      ]);

      for rpc in client.get_rpcs(1).values() {
         let _ = client.client_for(rpc, ClientKind::Standard).await.unwrap();
      }
      assert_eq!(client.clients.read().unwrap().len(), 2);

      // Simulate loading a provider set that no longer lists the second endpoint.
      client.write(|rpcs| {
         rpcs.get_mut(&1).unwrap().remove("http://127.0.0.1:2");
      });
      client.retain_known_clients();

      assert_eq!(client.clients.read().unwrap().len(), 1);
   }

   /// A cached fork belongs to the node that reported it, so any change to which endpoints a chain
   /// has — added, removed, enabled, disabled — must drop it.
   #[test]
   fn a_changed_endpoint_set_forgets_the_nodes_fork() {
      let client = client_with([http_rpc("http://127.0.0.1:1")]);
      let reported = |client: &ClientManager| {
         client.node_forks.write().unwrap().insert(
            1,
            NodeFork {
               spec: Some(SpecId::OSAKA),
               asked_at: Instant::now(),
               asked_at_unix: TimeStamp::now_as_secs().unwrap_or_default().timestamp(),
            },
         );
      };
      let cached = |client: &ClientManager| client.node_forks.read().unwrap().contains_key(&1);

      reported(&client);
      client.add_rpc(1, http_rpc("http://127.0.0.1:2"));
      assert!(!cached(&client));

      reported(&client);
      client.set_rpc_enabled(1, "http://127.0.0.1:1", false);
      assert!(!cached(&client));

      reported(&client);
      client.remove_rpc(1, Arc::from("http://127.0.0.1:1"));
      assert!(!cached(&client));
   }

   /// The activation boundary is a known timestamp, not a guess: an answer read before a fork Zeus
   /// knows the time of is superseded once the wall clock passes that fork, while an answer read at
   /// or after it is about the fork now running. A chain with no activation time has no boundary, so
   /// only the elapsed-time TTL ages its answer there.
   #[test]
   fn a_fork_activation_expires_an_answer_read_before_it() {
      let sepolia = ChainId::EthereumSepolia.spec_activation_time().unwrap();

      // Read a minute before the fork, seen a minute after: the answer is about the old fork.
      assert!(answer_predates_activation(
         sepolia - 60,
         sepolia + 60,
         Some(sepolia)
      ));

      // The activation is the boundary, not the moment after it: at `now == activation` an entry
      // minted before it is already superseded.
      assert!(answer_predates_activation(
         sepolia - 1,
         sepolia,
         Some(sepolia)
      ));

      // Read after the activation: about the fork the chain is on, kept.
      assert!(!answer_predates_activation(
         sepolia + 1,
         sepolia + 60,
         Some(sepolia)
      ));
      // Read at the activation: the node was already answering about the new fork.
      assert!(!answer_predates_activation(
         sepolia,
         sepolia + 60,
         Some(sepolia)
      ));

      // No known activation (every L2): the boundary never fires, however old the entry is, and
      // the elapsed TTL is left to decide.
      assert!(!answer_predates_activation(0, u64::MAX, None));
   }

   /// A cached answer that postdates the chain's activation is served without a request, and the
   /// boundary test is what a pre-activation entry must fail. Ethereum's Osaka activation is in the
   /// past, so an entry minted now postdates it.
   #[tokio::test]
   async fn a_cached_node_spec_is_served_while_it_postdates_the_activation() {
      let client = client_with([]);
      let now = TimeStamp::now_as_secs().unwrap().timestamp();
      let activation = ChainId::Ethereum.spec_activation_time().unwrap();
      assert!(now >= activation);

      client.node_forks.write().unwrap().insert(
         1,
         NodeFork {
            spec: Some(SpecId::OSAKA),
            asked_at: Instant::now(),
            asked_at_unix: now,
         },
      );

      // No endpoint is configured, so a cache miss could only report `None`; `Some(OSAKA)` must
      // have come from the cache, on the fresh side of the activation.
      assert_eq!(client.node_spec(1).await, Some(SpecId::OSAKA));
   }

   /// The frame path cannot run async [`ClientManager::node_spec`], so the send screen's reserve
   /// reads the same cache synchronously. Nothing cached is `None` (the caller keeps the shipped
   /// spec), the node's answer is served as-is, and a cached "no answer" is still no answer.
   #[test]
   fn a_cached_node_spec_is_read_without_the_network() {
      let client = client_with([]);
      assert_eq!(client.cached_node_spec(1), None);
      assert_eq!(client.cached_node_spec(8453), None);

      client.node_forks.write().unwrap().insert(
         1,
         NodeFork {
            spec: Some(SpecId::AMSTERDAM),
            asked_at: Instant::now(),
            asked_at_unix: TimeStamp::now_as_secs().unwrap().timestamp(),
         },
      );
      assert_eq!(
         client.cached_node_spec(1),
         Some(SpecId::AMSTERDAM)
      );

      // A cached miss stays a miss: `node_spec` records "the node said nothing" too, and it must
      // not be mistaken for a usable answer.
      client.node_forks.write().unwrap().insert(
         1,
         NodeFork {
            spec: None,
            asked_at: Instant::now(),
            asked_at_unix: 0,
         },
      );
      assert_eq!(client.cached_node_spec(1), None);
   }

   /// A profile written before a chain existed names only the chains that did. The shipped endpoints
   /// used to be inserted for a fresh profile only, so a chain added in a later build was unusable
   /// out of the box for anyone upgrading, with no way to get its endpoints back.
   #[test]
   fn a_profile_gains_the_shipped_endpoints_for_chains_it_predates() {
      let mut rpcs: HashMap<u64, RpcMapByUrl> = HashMap::new();
      insert_chain_rpcs(&mut rpcs, 1, &["https://mine.example/eth"], &[]);

      insert_missing_builtin_rpcs(&mut rpcs);

      // What the profile did name is left exactly as the user configured it.
      assert_eq!(rpcs[&1].len(), 1);
      assert!(rpcs[&1].contains_key("https://mine.example/eth"));
      // Every chain Zeus ships endpoints for is present, with all of its endpoints.
      assert_eq!(rpcs.len(), BUILTIN_ENDPOINTS.len());
      for &(chain, urls, mev_urls) in BUILTIN_ENDPOINTS {
         // Chain 1 is the one the profile named, whose single endpoint is asserted above.
         if chain == 1 {
            continue;
         }
         assert_eq!(rpcs[&chain].len(), urls.len() + mev_urls.len());
      }
   }

   #[test]
   fn rpc_limits_default_when_missing_from_json() {
      let rpc = Rpc::builder("http://127.0.0.1:1", 1).build();
      let mut value: serde_json::Value =
         serde_json::from_str(&serde_json::to_string(&rpc).unwrap()).unwrap();
      let obj = value.as_object_mut().unwrap();
      obj.remove("rps");
      obj.remove("cu_per_second");

      let rpc: Rpc = serde_json::from_value(value).unwrap();
      assert_eq!(rpc.rps, DEFAULT_RPC_RPS);
      assert_eq!(rpc.cu_per_second, DEFAULT_RPC_CU_PER_SECOND);
   }

   #[test]
   fn clamp_limits_bounds_zero_and_max() {
      let mut rpc = Rpc::builder("http://127.0.0.1:1", 1).rps(0).cu_per_second(0).build();
      assert_eq!(rpc.rps, MIN_RPC_RPS);
      assert_eq!(rpc.cu_per_second, MIN_RPC_CU_PER_SECOND);

      rpc.rps = u32::MAX;
      rpc.cu_per_second = u64::MAX;
      rpc.clamp_limits();
      assert_eq!(rpc.rps, MAX_RPC_RPS);
      assert_eq!(rpc.cu_per_second, MAX_RPC_CU_PER_SECOND);
   }

   #[tokio::test]
   async fn set_rpc_limits_clamps_and_evicts() {
      let client = client_with([http_rpc("http://127.0.0.1:1")]);
      let rpc = client.get_best_rpc(1).unwrap();
      let _ = client.client_for(&rpc, ClientKind::Standard).await.unwrap();
      assert_eq!(client.clients.read().unwrap().len(), 1);

      client.set_rpc_limits(1, "http://127.0.0.1:1", 0, 0);
      assert!(client.clients.read().unwrap().is_empty());

      let rpc = client.get_rpcs(1).into_values().next().unwrap();
      assert_eq!(rpc.rps, MIN_RPC_RPS);
      assert_eq!(rpc.cu_per_second, MIN_RPC_CU_PER_SECOND);
   }

   #[test]
   fn client_settings_concurrency_clamps() {
      let client = ClientManager::default();
      assert_eq!(client.concurrency(), default_concurrency());

      client.set_concurrency(0);
      assert_eq!(client.concurrency(), MIN_CONCURRENCY);

      client.set_concurrency(usize::MAX);
      assert_eq!(client.concurrency(), MAX_CONCURRENCY);

      client.set_concurrency(4);
      assert_eq!(client.concurrency(), 4);
   }

   #[test]
   fn client_settings_default_when_field_missing() {
      let settings: ClientSettings = serde_json::from_str("{}").unwrap();
      assert_eq!(settings.concurrency, default_concurrency());
   }

   #[test]
   fn client_kind_timeout_secs_maps_each_policy() {
      assert_eq!(
         ClientKind::Standard.timeout_secs(),
         CLIENT_TIMEOUT
      );
      assert_eq!(ClientKind::Short.timeout_secs(), 10);
      assert_eq!(
         ClientKind::Send.timeout_secs(),
         CLIENT_TIMEOUT_FOR_SENDING_TX
      );
   }

   #[tokio::test]
   async fn client_for_caches_one_connection_per_endpoint() {
      let client = client_with([http_rpc("http://127.0.0.1:1")]);
      let rpc = client.get_best_rpc(1).unwrap();
      let _a = client.client_for(&rpc, ClientKind::Standard).await.unwrap();
      let _b = client.client_for(&rpc, ClientKind::Standard).await.unwrap();
      assert_eq!(client.clients.read().unwrap().len(), 1);
   }

   #[tokio::test]
   async fn get_client_reuses_the_cached_connection() {
      let client = client_with([http_rpc("http://127.0.0.1:1")]);
      let _a = client.get_client(1).await.unwrap();
      let _b = client.get_client(1).await.unwrap();
      assert_eq!(client.clients.read().unwrap().len(), 1);
   }

   #[tokio::test]
   async fn different_kinds_do_not_share_a_connection() {
      let client = client_with([http_rpc("http://127.0.0.1:1")]);
      let rpc = client.get_best_rpc(1).unwrap();
      let _standard = client.client_for(&rpc, ClientKind::Standard).await.unwrap();
      let _send = client.client_for(&rpc, ClientKind::Send).await.unwrap();
      assert_eq!(client.clients.read().unwrap().len(), 2);
   }

   #[tokio::test]
   async fn evict_client_drops_the_cached_connection() {
      let client = client_with([http_rpc("http://127.0.0.1:1")]);
      let rpc = client.get_best_rpc(1).unwrap();
      let _ = client.client_for(&rpc, ClientKind::Standard).await.unwrap();
      assert_eq!(client.clients.read().unwrap().len(), 1);

      client.evict_client(1, "http://127.0.0.1:1");
      assert!(client.clients.read().unwrap().is_empty());
   }

   #[test]
   fn get_best_rpc_prefers_fully_functional_over_faster_partial() {
      let client = client_with([
         rpc_for_select("http://partial", false, 10),
         rpc_for_select("http://full", true, 100),
      ]);
      let best = client.get_best_rpc(1).unwrap();
      assert_eq!(&*best.url, "http://full");
   }

   #[test]
   fn get_best_rpc_falls_back_to_partial_when_none_are_fully_functional() {
      let client = client_with([
         rpc_for_select("http://slow", false, 80),
         rpc_for_select("http://fast", false, 10),
      ]);
      let best = client.get_best_rpc(1).unwrap();
      assert_eq!(&*best.url, "http://fast");
   }

   #[test]
   fn get_best_rpc_ignores_the_usage_cooldown_across_classes() {
      // The fully-functional endpoint was used a moment ago, so it carries the whole cooldown
      // penalty — and still beats a partial endpoint whose score is its latency alone.
      let mut fully = rpc_for_select("http://full", true, 500);
      fully.rps = 1;
      fully.last_used = TimeStamp::now_as_millis().unwrap_or_default().timestamp();

      let client = client_with([fully, rpc_for_select("http://partial", false, 1)]);

      assert_eq!(
         &*client.get_best_rpc(1).unwrap().url,
         "http://full"
      );
   }

   #[test]
   fn get_best_rpc_ignores_the_failure_penalty_across_classes() {
      // A failure a second ago adds up to 10_000 ms to the score; it ranks endpoints inside a
      // class and nothing more, so the partial endpoint stays off the wire.
      let mut fully = rpc_for_select("http://full", true, 500);
      fully.last_failure = Some(TimeStamp::now_as_millis().unwrap_or_default().timestamp());

      let client = client_with([fully, rpc_for_select("http://partial", false, 1)]);

      assert_eq!(
         &*client.get_best_rpc(1).unwrap().url,
         "http://full"
      );
   }

   #[test]
   fn get_best_rpc_load_balances_inside_the_fully_functional_class() {
      // Both are fully functional, so the score decides: the endpoint used a moment ago loses to
      // the idle one for as long as its cooldown lasts.
      let mut used = rpc_for_select("http://used", true, 10);
      used.rps = 1;
      used.last_used = TimeStamp::now_as_millis().unwrap_or_default().timestamp();

      let client = client_with([used, rpc_for_select("http://idle", true, 80)]);

      assert_eq!(
         &*client.get_best_rpc(1).unwrap().url,
         "http://idle"
      );
   }

   fn rpc_checked(url: &str, fully: bool) -> Rpc {
      let mut rpc = rpc_for_select(url, fully, 10);
      rpc.check.last_check = Some(1);
      rpc
   }

   #[test]
   fn rpcs_fully_functional_ignores_unchecked_and_disabled() {
      let mut unchecked = rpc_for_select("http://unchecked", false, 10);
      unchecked.check.last_check = None;
      let mut disabled = rpc_checked("http://disabled-partial", false);
      disabled.enabled = false;
      let client = client_with([unchecked, disabled, rpc_checked("http://ok", true)]);
      assert!(client.rpcs_fully_functional(1));
   }

   #[test]
   fn rpcs_fully_functional_false_when_a_checked_enabled_rpc_is_partial() {
      let client = client_with([
         rpc_checked("http://ok", true),
         rpc_checked("http://partial", false),
      ]);
      assert!(!client.rpcs_fully_functional(1));
   }

   /// An enabled, already-checked endpoint that answered exactly `supported`'s probes.
   fn rpc_supporting(url: &str, supported: impl IntoIterator<Item = RpcFunctionality>) -> Rpc {
      let supported: Vec<RpcFunctionality> = supported.into_iter().collect();
      let has = |functionality: RpcFunctionality| supported.contains(&functionality);

      let mut rpc = http_rpc(url);
      rpc.check.last_check = Some(1);
      rpc.check.multicall = has(RpcFunctionality::Multicall);
      rpc.check.state_override = has(RpcFunctionality::StateOverride);
      rpc.check.json_rpc_batch = has(RpcFunctionality::JsonRpcBatch);
      if has(RpcFunctionality::ZeusStateView) {
         rpc.check.v2_pool_reserves_batch = V2_POOL_RESERVES_BATCH;
      }
      rpc
   }

   /// An enabled, working archive endpoint (`rpc_checked` plus the archive probe).
   fn rpc_archive(url: &str, fully: bool) -> Rpc {
      let mut rpc = rpc_checked(url, fully);
      rpc.check.archive = true;
      rpc
   }

   #[test]
   fn unavailable_functionalities_is_empty_when_endpoints_cover_each_other() {
      let client = client_with([
         rpc_supporting("http://a", [RpcFunctionality::Multicall]),
         rpc_supporting(
            "http://b",
            [
               RpcFunctionality::StateOverride,
               RpcFunctionality::JsonRpcBatch,
               RpcFunctionality::ZeusStateView,
            ],
         ),
      ]);

      assert!(client.unavailable_functionalities(1).is_empty());
   }

   #[test]
   fn unavailable_functionalities_names_what_no_endpoint_can_do() {
      let client = client_with([
         rpc_supporting(
            "http://a",
            [
               RpcFunctionality::Multicall,
               RpcFunctionality::JsonRpcBatch,
               RpcFunctionality::ZeusStateView,
            ],
         ),
         rpc_supporting("http://b", []),
      ]);

      assert_eq!(
         client.unavailable_functionalities(1),
         vec![RpcFunctionality::StateOverride]
      );
   }

   #[test]
   fn unavailable_functionalities_ignores_unchecked_and_disabled_endpoints() {
      let unchecked = http_rpc("http://unchecked");
      let mut disabled = rpc_supporting("http://disabled", []);
      disabled.enabled = false;

      let client = client_with([unchecked, disabled]);

      assert!(client.unavailable_functionalities(1).is_empty());
   }

   #[test]
   fn stateview_is_available_when_only_one_batch_op_answers() {
      let mut rpc = http_rpc("http://a");
      rpc.check.last_check = Some(1);
      rpc.check.validate_v4_pools_batch = VALIDATE_V4_POOLS_BATCH;

      let client = client_with([rpc]);

      assert!(!client.unavailable_functionalities(1).contains(&RpcFunctionality::ZeusStateView));
   }

   #[test]
   fn archive_candidates_put_fully_functional_endpoints_first() {
      // Fed in the wrong order on purpose: every caller hands these over in endpoint-map order,
      // so the sort is the only thing that can put the good endpoint in front.
      let candidates = archive_candidates(
         [
            rpc_archive("http://partial", false),
            rpc_archive("http://full", true),
         ],
         false,
      );

      assert_eq!(candidates.len(), 2);
      assert_eq!(&*candidates[0].url, "http://full");
   }

   #[test]
   fn archive_candidates_keep_only_archive_endpoints_that_can_answer() {
      let mut not_archive = rpc_checked("http://not-archive", true);
      not_archive.check.archive = false;
      let mut disabled = rpc_archive("http://disabled", true);
      disabled.enabled = false;
      let mut not_working = rpc_archive("http://not-working", true);
      not_working.check.working = false;

      let endpoints = [
         not_archive,
         disabled,
         not_working,
         rpc_archive("http://ok", true),
         rpc_archive("ws://ws-ok", true),
      ];

      let http_only: Vec<String> = archive_candidates(endpoints.clone(), true)
         .iter()
         .map(|rpc| rpc.url.to_string())
         .collect();
      assert_eq!(http_only, vec!["http://ok"]);

      // A websocket client is fine when the caller does not ask for http.
      assert_eq!(archive_candidates(endpoints, false).len(), 2);
   }

   #[tokio::test]
   async fn archive_client_dials_the_fully_functional_endpoint() {
      let client = client_with([
         rpc_archive("http://partial-1", false),
         rpc_archive("http://partial-2", false),
         rpc_archive("http://partial-3", false),
         rpc_archive("http://full", true),
      ]);

      client.get_archive_client(1, false).await.unwrap();

      let cached = client.clients.read().unwrap();
      assert_eq!(cached.len(), 1);
      assert!(cached.keys().any(|key| &*key.url == "http://full"));
   }

   #[tokio::test]
   async fn archive_client_falls_back_to_a_partial_archive_endpoint() {
      let client = client_with([rpc_archive("http://partial", false)]);

      client.get_archive_client(1, false).await.unwrap();

      let cached = client.clients.read().unwrap();
      assert!(cached.keys().any(|key| &*key.url == "http://partial"));
   }
}
