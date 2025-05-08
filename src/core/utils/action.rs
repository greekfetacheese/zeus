use crate::core::ZeusCtx;
use anyhow::anyhow;
use std::str::FromStr;
use zeus_eth::{
   abi::{erc20, protocols::across, uniswap, weth9},
   alloy_primitives::{Address, Bytes, Log, U256},
   alloy_provider::Provider,
   amm::{
      UniswapV2Pool, UniswapV3Pool,
      uniswap::{AnyUniswapPool, UniswapPool},
   },
   currency::{Currency, ERC20Token, NativeCurrency},
   dapps::Dapp,
   utils::NumericValue,
};

/// Enum to describe an action that happened or is about to happen on-chain
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum OnChainAction {
   /// Cross Swap / Bridge
   Bridge(BridgeParams),

   /// Token Swap
   SwapToken(SwapParams),

   /// ERC20 Token Approval
   TokenApprove(TokenApproveParams),

   Transfer(TransferParams),

   WrapETH(WrapETHParams),

   UnwrapWETH(UnwrapWETHParams),

   Other,
}

impl OnChainAction {
   pub fn dummy_token_approve() -> Self {
      let token = Currency::from(ERC20Token::weth());
      let amount = NumericValue::parse_to_wei("1", 18);
      let amount_usd = NumericValue::value(amount.f64(), 1600.0);
      let owner = Address::ZERO;
      let spender = Address::ZERO;

      let params = TokenApproveParams {
         token,
         amount,
         amount_usd: Some(amount_usd),
         owner,
         spender,
      };

      Self::TokenApprove(params)
   }

   pub fn dummy_swap() -> Self {
      let input_token = Currency::from(ERC20Token::weth());
      let output_token = Currency::from(ERC20Token::dai());
      let amount_in = NumericValue::parse_to_wei("1", 18);
      let amount_usd = NumericValue::value(amount_in.f64(), 1600.0);

      let params = SwapParams {
         dapp: Dapp::Uniswap,
         input_currency: input_token,
         output_currency: output_token,
         amount_in: amount_in.clone(),
         amount_in_usd: Some(amount_usd.clone()),
         received: amount_usd.clone(),
         received_usd: Some(amount_usd),
         sender: Address::ZERO,
         recipient: Some(Address::ZERO),
      };

      Self::SwapToken(params)
   }

   pub fn dummy_bridge() -> Self {
      let input_token = Currency::from(ERC20Token::weth());
      let output_token = Currency::from(ERC20Token::weth());
      let amount_in = NumericValue::parse_to_wei("1", 18);
      let amount_usd = NumericValue::value(amount_in.f64(), 1600.0);

      let params = BridgeParams {
         dapp: Dapp::Across,
         origin_chain: 1,
         destination_chain: 10,
         input_currency: input_token,
         output_currency: output_token,
         amount: amount_in.clone(),
         amount_usd: Some(amount_usd.clone()),
         received: amount_in.clone(),
         received_usd: Some(amount_usd),
         sender: Address::ZERO,
         recipient: Address::ZERO,
      };

      Self::Bridge(params)
   }

   pub fn dummy_transfer() -> Self {
      let currency = Currency::from(ERC20Token::weth());
      let amount = NumericValue::parse_to_wei("1", 18);
      let amount_usd = NumericValue::value(amount.f64(), 1600.0);

      let params = TransferParams {
         currency,
         amount,
         amount_usd: Some(amount_usd),
         sender: Address::ZERO,
         recipient: Address::ZERO,
      };

      Self::Transfer(params)
   }

   pub fn new_transfer(
      currency: Currency,
      amount: NumericValue,
      amount_usd: NumericValue,
      sender: Address,
      recipient: Address,
   ) -> Self {
      let params = TransferParams {
         currency,
         amount,
         amount_usd: Some(amount_usd),
         sender,
         recipient,
      };

      Self::Transfer(params)
   }

   pub fn new_bridge(
      dapp: Dapp,
      origin_chain: u64,
      destination_chain: u64,
      input_currency: Currency,
      output_currency: Currency,
      amount_in: NumericValue,
      amount_usd: NumericValue,
      received: NumericValue,
      received_usd: NumericValue,
      sender: Address,
      recipient: Address,
   ) -> Self {
      let params = BridgeParams {
         dapp,
         origin_chain,
         destination_chain,
         input_currency,
         output_currency,
         amount: amount_in,
         amount_usd: Some(amount_usd),
         received,
         received_usd: Some(received_usd),
         sender,
         recipient,
      };

      Self::Bridge(params)
   }

   pub async fn new(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      interact_to: Address,
      call_data: Bytes,
      value: U256,
      logs: Vec<Log>,
   ) -> Self {
      if let Ok(params) = BridgeParams::new(
         ctx.clone(),
         chain,
         call_data.clone(),
         value,
         logs.clone(),
      )
      .await
      {
         return Self::Bridge(params);
      }

      if let Ok(params) = SwapParams::new(ctx.clone(), chain, from, logs.clone()).await {
         return Self::SwapToken(params);
      }

      if let Ok(params) = TransferParams::new(
         ctx.clone(),
         chain,
         from,
         interact_to,
         call_data.clone(),
         value,
      )
      .await
      {
         return Self::Transfer(params);
      }

      if let Ok(params) = TokenApproveParams::new(ctx.clone(), chain, call_data.clone(), logs.clone()).await
      {
         return Self::TokenApprove(params);
      }

      if let Ok(params) = WrapETHParams::new(ctx.clone(), chain, from, call_data.clone(), value, logs.clone()) {
         return Self::WrapETH(params);
      }

      if let Ok(params) = UnwrapWETHParams::new(ctx, chain, from, call_data, value, logs) {
         return Self::UnwrapWETH(params);
      }

      Self::Other
   }

   pub fn name(&self) -> &'static str {
      match self {
         Self::Bridge(_) => "Bridge",
         Self::SwapToken(_) => "Swap Token",
         Self::Transfer(_) => "Transfer",
         Self::TokenApprove(_) => "Token Approval",
         Self::WrapETH(_) => "Wrap ETH",
         Self::UnwrapWETH(_) => "Unwrap WETH",
         Self::Other => "Unknown interaction",
      }
   }

   pub fn token_approval_amount_str(&self) -> String {
      let amount = self.token_approval_params().amount;
      let unlimited = amount.wei2() == U256::MAX;
      if unlimited {
         return "Unlimited".to_string();
      } else {
         return amount.formatted().clone();
      }
   }

   /// Get the bridge params
   ///
   /// Panics if the action is not a bridge
   pub fn bridge_params(&self) -> BridgeParams {
      match self {
         Self::Bridge(params) => params.clone(),
         _ => panic!("Action is not a bridge"),
      }
   }

   /// Get the swap params
   ///
   /// Panics if the action is not a swap
   pub fn swap_params(&self) -> SwapParams {
      match self {
         Self::SwapToken(params) => params.clone(),
         _ => panic!("Action is not a swap"),
      }
   }

   /// Get the transfer params
   ///
   /// Panics if the action is not a transfer
   pub fn transfer_params(&self) -> TransferParams {
      match self {
         Self::Transfer(params) => params.clone(),
         _ => panic!("Action is not a transfer"),
      }
   }

   /// Get the token approval params
   ///
   /// Panics if the action is not a token approval
   pub fn token_approval_params(&self) -> TokenApproveParams {
      match self {
         Self::TokenApprove(params) => params.clone(),
         _ => panic!("Action is not a token approval"),
      }
   }

   /// Get the wrap eth params
   ///
   /// Panics if the action is not a wrap eth
   pub fn wrap_eth_params(&self) -> WrapETHParams {
      match self {
         Self::WrapETH(params) => params.clone(),
         _ => panic!("Action is not a wrap eth"),
      }
   }

   /// Get the unwrap eth params
   ///
   /// Panics if the action is not a unwrap eth
   pub fn unwrap_eth_params(&self) -> UnwrapWETHParams {
      match self {
         Self::UnwrapWETH(params) => params.clone(),
         _ => panic!("Action is not a unwrap eth"),
      }
   }

   pub fn is_bridge(&self) -> bool {
      matches!(self, Self::Bridge(_))
   }

   pub fn is_swap(&self) -> bool {
      matches!(self, Self::SwapToken(_))
   }

   pub fn is_transfer(&self) -> bool {
      matches!(self, Self::Transfer(_))
   }

   pub fn is_token_approval(&self) -> bool {
      matches!(self, Self::TokenApprove(_))
   }

   pub fn is_wrap_eth(&self) -> bool {
      matches!(self, Self::WrapETH(_))
   }

   pub fn is_unwrap_eth(&self) -> bool {
      matches!(self, Self::UnwrapWETH(_))
   }

   pub fn is_other(&self) -> bool {
      matches!(self, Self::Other)
   }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BridgeParams {
   pub dapp: Dapp,
   pub origin_chain: u64,
   pub destination_chain: u64,
   pub input_currency: Currency,
   pub output_currency: Currency,
   pub amount: NumericValue,
   /// USD value at the time of the tx
   pub amount_usd: Option<NumericValue>,
   pub received: NumericValue,
   /// USD value at the time of the tx
   pub received_usd: Option<NumericValue>,
   pub sender: Address,
   pub recipient: Address,
}

impl Default for BridgeParams {
   fn default() -> Self {
      Self {
         dapp: Dapp::Across,
         origin_chain: 1,
         destination_chain: 10,
         input_currency: Currency::default(),
         output_currency: Currency::default(),
         amount: NumericValue::default(),
         amount_usd: None,
         received: NumericValue::default(),
         received_usd: None,
         sender: Address::default(),
         recipient: Address::default(),
      }
   }
}

impl BridgeParams {
   pub async fn new(
      ctx: ZeusCtx,
      chain: u64,
      call_data: Bytes,
      _value: U256,
      logs: Vec<Log>,
   ) -> Result<Self, anyhow::Error> {
      let selector = call_data.get(0..4).unwrap_or_default();
      if selector == across::deposit_v3_selector() {
         match Self::from_across(ctx, chain, call_data, logs).await {
            Ok(params) => Ok(params),
            Err(e) => {
               tracing::error!("Failed to decode across bridge params: {:?}", e);
               Err(anyhow!("Failed to decode across bridge params"))
            }
         }
      } else {
         tracing::debug!("Call is not a bridge");
         return Err(anyhow!("Call is not a bridge"));
      }
   }

   /// Across Bridge Protocol
   ///
   /// https://across.to/
   pub async fn from_across(
      ctx: ZeusCtx,
      origin_chain: u64,
      call_data: Bytes,
      logs: Vec<Log>,
   ) -> Result<Self, anyhow::Error> {
      let decoded = across::decode_deposit_v3_call(&call_data)?;
      let dest_chain = decoded.destinationChainId.try_into()?;
      let input_cached = ctx.read(|ctx| ctx.currency_db.get_erc20_token(origin_chain, decoded.inputToken));
      let output_cached =
         ctx.read(|ctx| ctx.currency_db.get_erc20_token(dest_chain, decoded.outputToken));


      let input_token = if let Some(token) = input_cached {
         token
      } else {
         let client = ctx.get_client_with_id(dest_chain)?;
         let token = ERC20Token::new(client.clone(), decoded.inputToken, dest_chain).await?;
         ctx.write(|ctx| {
            ctx.currency_db
               .insert_currency(dest_chain, Currency::from(token.clone()))
         });
         ctx.save_currency_db();
         token
      };

      let output_token = if let Some(token) = output_cached {
         token
      } else {
         let client = ctx.get_client_with_id(dest_chain)?;
         let token = ERC20Token::new(client.clone(), decoded.outputToken, dest_chain).await?;
         ctx.write(|ctx| {
            ctx.currency_db
               .insert_currency(dest_chain, Currency::from(token.clone()))
         });
         ctx.save_currency_db();
         token
      };

      // Output amount based on the logs, we could also use the amount from the decoded call data
      // But i think this is more reliable in case something goes wrong
      let mut decode_log = None;
      for log in logs {
         if let Ok(decoded) = across::decode_funds_deposited_log(&log) {
            decode_log = Some(decoded);
            break;
         }
      }

      if decode_log.is_none() {
         return Err(anyhow!("Failed to decode funds deposited log"));
      }

      let decoded_log = decode_log.unwrap();

      let amount = NumericValue::format_wei(decoded.inputAmount, input_token.decimals);
      let amount_usd = ctx.get_currency_value2(amount.f64(), &Currency::from(input_token.clone()));
      let received = NumericValue::format_wei(decoded_log.output_amount, output_token.decimals);
      let received_usd = ctx.get_currency_value2(
         received.f64(),
         &Currency::from(output_token.clone()),
      );

      let params = BridgeParams {
         dapp: Dapp::Across,
         origin_chain,
         destination_chain: dest_chain,
         input_currency: Currency::from(input_token),
         output_currency: Currency::from(output_token),
         amount,
         amount_usd: Some(amount_usd),
         received,
         received_usd: Some(received_usd),
         sender: decoded.depositor,
         recipient: decoded.recipient,
      };
      Ok(params)
   }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
/// USD values are the time of the tx
pub struct SwapParams {
   pub dapp: Dapp,
   pub input_currency: Currency,
   pub output_currency: Currency,
   pub amount_in: NumericValue,
   pub amount_in_usd: Option<NumericValue>,
   pub received: NumericValue,
   pub received_usd: Option<NumericValue>,
   pub sender: Address,
   pub recipient: Option<Address>,
}

impl Default for SwapParams {
   fn default() -> Self {
      Self {
         dapp: Dapp::Uniswap,
         input_currency: Currency::default(),
         output_currency: Currency::default(),
         amount_in: NumericValue::default(),
         amount_in_usd: None,
         received: NumericValue::default(),
         received_usd: None,
         sender: Address::default(),
         recipient: None,
      }
   }
}

impl SwapParams {
   pub async fn new(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      logs: Vec<Log>,
   ) -> Result<Self, anyhow::Error> {
      // if there is multiple swaps make sure to identiafy the start currenct and the end currency
      let mut swaps = Vec::new();
      for log in logs {
         if let Ok(params) = Self::from_uniswap_v2(ctx.clone(), chain, from, log.clone()).await {
            swaps.push(params);
         }

         if let Ok(params) = Self::from_uniswap_v3(ctx.clone(), chain, from, log.clone()).await {
            swaps.push(params);
         }
      }

      if swaps.is_empty() {
         return Err(anyhow::anyhow!("No swap logs found"));
      }

      let mut dapp = Dapp::Uniswap;
      let mut input_currency = Currency::default();
      let mut output_currency = Currency::default();
      let mut amount_in = NumericValue::default();
      let mut amount_in_usd = None;
      let mut amount_out = NumericValue::default();
      let mut amount_out_usd = None;
      let mut sender = Address::ZERO;
      let mut recipient = None;
      let len = swaps.len();
      for (i, swap) in swaps.iter().enumerate() {
         let is_first = i == 0;
         let is_last = i == len - 1;

         if is_first {
            dapp = swap.dapp.clone();
            input_currency = swap.input_currency.clone();
            amount_in = swap.amount_in.clone();
            amount_in_usd = swap.amount_in_usd.clone();
            sender = swap.sender;
         }

         if is_last {
            output_currency = swap.output_currency.clone();
            amount_out = swap.received.clone();
            amount_out_usd = swap.received_usd.clone();
            recipient = swap.recipient;
         }
      }

      let swap_params = SwapParams {
         dapp,
         input_currency,
         output_currency,
         amount_in,
         amount_in_usd,
         received: amount_out,
         received_usd: amount_out_usd,
         sender,
         recipient,
      };

      Ok(swap_params)
   }

   pub async fn from_uniswap_v2(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      log: Log,
   ) -> Result<Self, anyhow::Error> {
      let (swap_log, pool_address) = if let Ok(decoded) = uniswap::v2::pool::decode_swap_log(&log) {
         (decoded, log.address)
      } else {
         return Err(anyhow::anyhow!("Log is not a UniswapV2 swap log"));
      };

      let client = ctx.get_client_with_id(chain)?;
      let cached = ctx.read(|ctx| {
         ctx.pool_manager
            .get_v2_pool_from_address(chain, pool_address)
      });

      let pool = if let Some(pool) = cached {
         pool
      } else {
         let pool = UniswapV2Pool::from_address(client.clone(), chain, pool_address).await?;
         let pool = AnyUniswapPool::from_pool(pool);
         ctx.write(|ctx| ctx.pool_manager.add_pool(pool.clone()));
         ctx.write(|ctx| {
            ctx.currency_db
               .insert_currency(chain, pool.currency0().clone())
         });
         ctx.write(|ctx| {
            ctx.currency_db
               .insert_currency(chain, pool.currency1().clone())
         });
         pool
      };

      let (amount_in, token_in) = if swap_log.amount0In > swap_log.amount1In {
         (
            swap_log.amount0In,
            pool.currency0().to_erc20().into_owned(),
         )
      } else {
         (
            swap_log.amount1In,
            pool.currency1().to_erc20().into_owned(),
         )
      };

      let (amount_out, token_out) = if swap_log.amount0Out > swap_log.amount1Out {
         (
            swap_log.amount0Out,
            pool.currency0().to_erc20().into_owned(),
         )
      } else {
         (
            swap_log.amount1Out,
            pool.currency1().to_erc20().into_owned(),
         )
      };

      let amount_in = NumericValue::format_wei(amount_in, token_in.decimals);
      let amount_in_usd =
         ctx.get_currency_value2(amount_in.f64(), &Currency::from(token_in.clone()));
      let amount_out = NumericValue::format_wei(amount_out, token_out.decimals);
      let amount_out_usd = ctx.get_currency_value2(
         amount_out.f64(),
         &Currency::from(token_out.clone()),
      );
      let token_in = Currency::from(token_in);
      let token_out = Currency::from(token_out);

      let params = SwapParams {
         dapp: Dapp::Uniswap,
         input_currency: token_in,
         output_currency: token_out,
         amount_in,
         amount_in_usd: Some(amount_in_usd),
         received: amount_out,
         received_usd: Some(amount_out_usd),
         sender: from,
         recipient: None,
      };

      Ok(params)
   }

   pub async fn from_uniswap_v3(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      log: Log,
   ) -> Result<Self, anyhow::Error> {
      let (swap_log, pool_address) = if let Ok(decoded) = uniswap::v3::pool::decode_swap_log(&log) {
         (decoded, log.address)
      } else {
         return Err(anyhow::anyhow!("Log is not a UniswapV3 swap log"));
      };

      let client = ctx.get_client_with_id(chain)?;
      let cached = ctx.read(|ctx| {
         ctx.pool_manager
            .get_v3_pool_from_address(chain, pool_address)
      });

      let pool = if let Some(pool) = cached {
         pool
      } else {
         let pool = UniswapV3Pool::from_address(client, chain, pool_address).await?;
         let pool = AnyUniswapPool::from_pool(pool);
         ctx.write(|ctx| ctx.pool_manager.add_pool(pool.clone()));
         ctx.write(|ctx| {
            ctx.currency_db
               .insert_currency(chain, pool.currency0().clone())
         });
         ctx.write(|ctx| {
            ctx.currency_db
               .insert_currency(chain, pool.currency1().clone())
         });
         pool
      };

      let (amount_in, token_in) = if swap_log.amount0.is_positive() {
         (
            swap_log.amount0,
            pool.currency0().to_erc20().into_owned(),
         )
      } else {
         (
            swap_log.amount1,
            pool.currency1().to_erc20().into_owned(),
         )
      };

      let (amount_out, token_out) = if swap_log.amount1.is_negative() {
         (
            swap_log.amount1,
            pool.currency1().to_erc20().into_owned(),
         )
      } else {
         (
            swap_log.amount0,
            pool.currency0().to_erc20().into_owned(),
         )
      };

      let amount_in = U256::from_str(&amount_in.to_string())?;
      // remove the - sign
      let amount_out = amount_out
         .to_string()
         .trim_start_matches('-')
         .parse::<U256>()?;

      let amount_in = NumericValue::format_wei(amount_in, token_in.decimals);
      let amount_in_usd =
         ctx.get_currency_value2(amount_in.f64(), &Currency::from(token_in.clone()));
      let amount_out = NumericValue::format_wei(amount_out, token_out.decimals);
      let amount_out_usd = ctx.get_currency_value2(
         amount_out.f64(),
         &Currency::from(token_out.clone()),
      );
      let token_in = Currency::from(token_in);
      let token_out = Currency::from(token_out);

      let params = SwapParams {
         dapp: Dapp::Uniswap,
         input_currency: token_in,
         output_currency: token_out,
         amount_in,
         amount_in_usd: Some(amount_in_usd),
         received: amount_out,
         received_usd: Some(amount_out_usd),
         sender: from,
         recipient: None,
      };

      Ok(params)
   }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TransferParams {
   pub currency: Currency,
   pub amount: NumericValue,
   /// USD value at the time of the tx
   pub amount_usd: Option<NumericValue>,
   pub sender: Address,
   pub recipient: Address,
}

impl TransferParams {
   pub async fn new(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      interact_to: Address,
      call_data: Bytes,
      value: U256,
   ) -> Result<Self, anyhow::Error> {
      let client = ctx.get_client_with_id(chain)?;
      // TODO: Cache the bytecode
      let code = client.get_code_at(interact_to).await?;
      let selector = call_data.get(0..4).unwrap_or_default();

      if call_data.len() == 0 && code.len() == 0 {
         // Native currency transfer
         let native = NativeCurrency::from_chain_id(chain)?;
         let currency = Currency::from(native);
         let amount = NumericValue::format_wei(value, currency.decimals());
         let amount_usd = ctx.get_currency_value2(amount.f64(), &currency);

         Ok(Self {
            currency,
            amount,
            amount_usd: Some(amount_usd),
            sender: from,
            recipient: interact_to,
         })
      } else if selector == erc20::transfer_selector() {
         // ERC20 transfer
         let (recipient, amount) = erc20::decode_transfer_call(&call_data)?;
         let cached = ctx.read(|ctx| ctx.currency_db.get_erc20_token(chain, interact_to));

         let token = if let Some(token) = cached {
            token
         } else {
            let token = ERC20Token::new(client.clone(), interact_to, chain).await?;
            ctx.write(|ctx| {
               ctx.currency_db
                  .insert_currency(chain, Currency::from(token.clone()))
            });
            token
         };

         let amount = NumericValue::format_wei(amount, token.decimals);
         let amount_usd = ctx.get_currency_value2(amount.f64(), &Currency::from(token.clone()));

         Ok(Self {
            currency: Currency::from(token),
            amount,
            amount_usd: Some(amount_usd),
            sender: from,
            recipient,
         })
      } else {
         return Err(anyhow!("Not a transfer"));
      }
   }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TokenApproveParams {
   pub token: Currency,
   pub amount: NumericValue,
   pub amount_usd: Option<NumericValue>,
   pub owner: Address,
   pub spender: Address,
}

impl TokenApproveParams {
   pub async fn new(
      ctx: ZeusCtx,
      chain: u64,
      call_data: Bytes,
      logs: Vec<Log>,
   ) -> Result<Self, anyhow::Error> {
      let selector = call_data.get(0..4).unwrap_or_default();
      if selector != erc20::approve_selector() {
         return Err(anyhow!("Call is not an approve"));
      }

      let mut decoded = None;
      let mut token_addr = None;
      for log in logs {
         if let Ok(decoded_log) = erc20::decode_approve_log(&log) {
            decoded = Some(decoded_log);
            token_addr = Some(log.address);
            break;
         }
      }

      if decoded.is_none() {
         return Err(anyhow!("Failed to decode approve log"));
      }

      let decoded = decoded.unwrap();
      let token_addr = token_addr.unwrap();
      let client = ctx.get_client_with_id(chain)?;
      let cached = ctx.read(|ctx| ctx.currency_db.get_erc20_token(chain, token_addr));

      let token = if let Some(token) = cached {
         token
      } else {
         let token = ERC20Token::new(client, token_addr, chain).await?;
         ctx.write(|ctx| {
            ctx.currency_db
               .insert_currency(chain, Currency::from(token.clone()))
         });
         token
      };

      let amount = NumericValue::format_wei(decoded.value, token.decimals);
      let amount_usd = ctx.get_currency_value2(amount.f64(), &Currency::from(token.clone()));
      let owner = decoded.owner;
      let spender = decoded.spender;

      let params = TokenApproveParams {
         token: Currency::from(token),
         amount,
         amount_usd: Some(amount_usd),
         owner,
         spender,
      };

      Ok(params)
   }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WrapETHParams {
   pub from: Address,
   pub eth_amount: NumericValue,
   pub eth_amount_usd: Option<NumericValue>,
   pub weth_amount: NumericValue,
   pub weth_amount_usd: Option<NumericValue>,
}

impl WrapETHParams {
   pub fn new(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      call_data: Bytes,
      value: U256,
      logs: Vec<Log>,
   ) -> Result<Self, anyhow::Error> {
      let selector = call_data.get(0..4).unwrap_or_default();
      if selector != weth9::deposit_selector() {
         return Err(anyhow::anyhow!("Call is not a WETH deposit"));
      }

      let mut decoded = None;
      for log in &logs {
         if let Ok(decoded_log) = weth9::decode_deposit_log(log) {
            decoded = Some(decoded_log);
            break;
         }
      }

      let decoded = decoded.ok_or(anyhow::anyhow!("Failed to decode deposit log"))?;

      let currency = Currency::from(NativeCurrency::from_chain_id(chain).unwrap());
      let eth_amount = NumericValue::format_wei(value, currency.decimals());
      let eth_amount_usd = ctx.get_currency_value2(eth_amount.f64(), &currency);
      let weth_amount = NumericValue::format_wei(decoded.wad, currency.decimals());
      let weth_amount_usd = ctx.get_currency_value2(weth_amount.f64(), &currency);

      Ok(Self {
         from,
         eth_amount,
         eth_amount_usd: Some(eth_amount_usd),
         weth_amount,
         weth_amount_usd: Some(weth_amount_usd),
      })
   }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UnwrapWETHParams {
   pub from: Address,
   pub weth_amount: NumericValue,
   pub weth_amount_usd: Option<NumericValue>,
   pub eth_amount: NumericValue,
   pub eth_amount_usd: Option<NumericValue>,
}

impl UnwrapWETHParams {
   pub fn new(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      call_data: Bytes,
      value: U256,
      logs: Vec<Log>,
   ) -> Result<Self, anyhow::Error> {
      let selector = call_data.get(0..4).unwrap_or_default();
      if selector != weth9::withdraw_selector() {
         return Err(anyhow::anyhow!("Call is not a WETH withdraw"));
      }

      let mut decoded = None;
      for log in &logs {
         if let Ok(decoded_log) = weth9::decode_withdraw_log(log) {
            decoded = Some(decoded_log);
            break;
         }
      }

      if decoded.is_none() {
         return Err(anyhow::anyhow!("Failed to decode withdraw log"));
      }

      let decoded = decoded.unwrap();

      let currency = Currency::from(NativeCurrency::from_chain_id(chain).unwrap());
      let weth_amount = NumericValue::format_wei(value, currency.decimals());
      let weth_amount_usd = ctx.get_currency_value2(weth_amount.f64(), &currency);
      let eth_amount = NumericValue::format_wei(decoded.wad, currency.decimals());
      let eth_amount_usd = ctx.get_currency_value2(eth_amount.f64(), &currency);

      Ok(Self {
         from,
         weth_amount,
         weth_amount_usd: Some(weth_amount_usd),
         eth_amount,
         eth_amount_usd: Some(eth_amount_usd),
      })
   }
}
