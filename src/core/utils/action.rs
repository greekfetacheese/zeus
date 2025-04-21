use crate::core::ZeusCtx;
use anyhow::bail;
use std::str::FromStr;
use zeus_eth::{
   abi::{erc20, protocols::across, uniswap},
   alloy_primitives::{Address, Bytes, Log, U256},
   alloy_provider::Provider,
   amm::{UniswapV2Pool, UniswapV3Pool},
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
      let mut action = Self::Other;

      if let Ok(params) = BridgeParams::new(
         ctx.clone(),
         chain,
         call_data.clone(),
         value,
         logs.clone(),
      )
      .await
      {
         action = Self::Bridge(params);
      }

      if let Ok(params) = SwapParams::new(ctx.clone(), chain, from, logs.clone()).await {
         action = Self::SwapToken(params);
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
         action = Self::Transfer(params);
      }

      if let Ok(params) = TokenApproveParams::new(ctx, chain, call_data, logs).await {
         action = Self::TokenApprove(params);
      }

      action
   }

   pub fn name(&self) -> &'static str {
      match self {
         Self::Bridge(_) => "Bridge",
         Self::SwapToken(_) => "Swap Token",
         Self::Transfer(_) => "Transfer",
         Self::TokenApprove(_) => "Token Approval",
         Self::Other => "Unknown interaction",
      }
   }

   /// Get the currency to be payed
   ///
   /// Eg. For a bridge & swap, it will be the `input_currency`
   /// 
   /// For a transfer, it will be the `currency` to be sent
   /// 
   /// For Token Approval, it will be the `token` to be approved
   pub fn input_currency(&self) -> Currency {
      match self {
         Self::Bridge(params) => params.input_currency.clone(),
         Self::SwapToken(params) => params.input_currency.clone(),
         Self::Transfer(params) => params.currency.clone(),
         Self::TokenApprove(params) => params.token.clone(),
         Self::Other => Currency::default(),
      }
   }

   /// Get the currency to be received
   ///
   /// Eg. For a bridge & swap, it will be the `output_currency`
   pub fn output_currency(&self) -> Currency {
      match self {
         Self::Bridge(params) => params.output_currency.clone(),
         Self::SwapToken(params) => params.output_currency.clone(),
         Self::Transfer(_) => Currency::default(),
         Self::TokenApprove(_) => Currency::default(),
         Self::Other => Currency::default(),
      }
   }

   /// Get the amount to be payed
   ///
   /// Eg. For a bridge & swap, it will be the `amount_in`
   /// 
   /// For a transfer, it will be the `amount`
   /// 
   /// For Token Approval, it will be the `amount` to be approved
   pub fn amount(&self) -> NumericValue {
      match self {
         Self::Bridge(params) => params.amount.clone(),
         Self::SwapToken(params) => params.amount_in.clone(),
         Self::Transfer(params) => params.amount.clone(),
         Self::TokenApprove(params) => params.amount.clone(),
         Self::Other => NumericValue::default(),
      }
   }

   pub fn token_approval_amount_str(&self) -> String {
      let amount = self.amount();
      let unlimited = amount.wei2() == U256::MAX;
      if unlimited {
         return "Unlimited".to_string()
      } else {
        return amount.formatted().clone()
      }
      }

   /// Get the amount usd value to be payed
   ///
   /// Eg. For a bridge & swap, it will be the `amount_in_usd`
   /// For a transfer, it will be the `amount_usd`
   pub fn amount_usd(&self) -> Option<NumericValue> {
      match self {
         Self::Bridge(params) => params.amount_usd.clone(),
         Self::SwapToken(params) => params.amount_in_usd.clone(),
         Self::Transfer(params) => params.amount_usd.clone(),
         Self::TokenApprove(params) => params.amount_usd.clone(),
         Self::Other => None,
      }
   }

   /// Get the amount to be received
   ///
   /// Eg. For a bridge & swap, it will be the `amount_out`
   pub fn received(&self) -> NumericValue {
      match self {
         Self::Bridge(params) => params.received.clone(),
         Self::SwapToken(params) => params.received.clone(),
         Self::Transfer(_) => NumericValue::default(),
         Self::TokenApprove(_) => NumericValue::default(),
         Self::Other => NumericValue::default(),
      }
   }

   /// Get the amount usd value to be received
   ///
   /// Eg. For a bridge & swap, it will be the `received_usd`
   pub fn received_usd(&self) -> Option<NumericValue> {
      match self {
         Self::Bridge(params) => params.received_usd.clone(),
         Self::SwapToken(params) => params.received_usd.clone(),
         Self::Transfer(_) => None,
         Self::TokenApprove(_) => None,
         Self::Other => None,
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
         Self::from_across(ctx, chain, call_data, logs).await
      } else {
         bail!("Call is not a bridge")
      }
   }

   /// Across Bridge Protocol
   ///
   /// https://across.to/
   pub async fn from_across(
      ctx: ZeusCtx,
      chain: u64,
      call_data: Bytes,
      logs: Vec<Log>,
   ) -> Result<Self, anyhow::Error> {
      let decoded = across::decode_deposit_v3_call(&call_data)?;
      let client = ctx.get_client_with_id(chain)?;
      let input_cached = ctx.read(|ctx| ctx.currency_db.get_erc20_token(chain, decoded.inputToken));
      let output_cached =
         ctx.read(|ctx| ctx.currency_db.get_erc20_token(chain, decoded.outputToken));

      let input_token = if let Some(token) = input_cached {
         token
      } else {
         let token = ERC20Token::new(client.clone(), decoded.inputToken, chain).await?;
         ctx.write(|ctx| {
            ctx.currency_db
               .insert_currency(chain, Currency::from(token.clone()))
         });
         token
      };

      let output_token = if let Some(token) = output_cached {
         token
      } else {
         let token = ERC20Token::new(client.clone(), decoded.outputToken, chain).await?;
         ctx.write(|ctx| {
            ctx.currency_db
               .insert_currency(chain, Currency::from(token.clone()))
         });
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
         bail!("Failed to decode funds deposited log");
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
         origin_chain: chain,
         destination_chain: decoded.destinationChainId.try_into()?,
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
      let params =
         if let Ok(params) = Self::from_uniswap_v2(ctx.clone(), chain, from, logs.clone()).await {
            params
         } else {
            Self::from_uniswap_v3(ctx, chain, from, logs).await?
         };

      Ok(params)
   }

   pub async fn from_uniswap_v2(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      logs: Vec<Log>,
   ) -> Result<Self, anyhow::Error> {
      let mut swap_log = None;
      let mut pool_address = Address::ZERO;
      for log in logs {
         if let Ok(decoded) = uniswap::v2::pool::decode_swap_log(&log) {
            swap_log = Some(decoded);
            pool_address = log.address;
         }
      }

      if swap_log.is_none() {
         bail!("Failed to decode swap log");
      }

      let swap_log = swap_log.unwrap();
      let client = ctx.get_client_with_id(chain)?;
      let cached = ctx.read(|ctx| {
         ctx.pool_manager
            .get_v2_pool_from_address(chain, pool_address)
      });

      let pool = if let Some(pool) = cached {
         pool
      } else {
         let pool = UniswapV2Pool::from_address(client.clone(), chain, pool_address).await?;
         ctx.write(|ctx| ctx.pool_manager.add_v2_pools(vec![pool.clone()]));
         ctx.write(|ctx| {
            ctx.currency_db
               .insert_currency(chain, Currency::from(pool.token0.clone()))
         });
         ctx.write(|ctx| {
            ctx.currency_db
               .insert_currency(chain, Currency::from(pool.token1.clone()))
         });
         pool
      };

      let (amount_in, token_in) = if swap_log.amount0In > swap_log.amount1In {
         (swap_log.amount0In, pool.token0.clone())
      } else {
         (swap_log.amount1In, pool.token1.clone())
      };

      let (amount_out, token_out) = if swap_log.amount0Out > swap_log.amount1Out {
         (swap_log.amount0Out, pool.token0.clone())
      } else {
         (swap_log.amount1Out, pool.token1.clone())
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
      logs: Vec<Log>,
   ) -> Result<Self, anyhow::Error> {
      let mut swap_log = None;
      let mut pool_address = Address::ZERO;
      for log in logs {
         if let Ok(decoded) = uniswap::v3::pool::decode_swap_log(&log) {
            swap_log = Some(decoded);
            pool_address = log.address;
         }
      }

      if swap_log.is_none() {
         bail!("Failed to decode swap log");
      }

      let swap_log = swap_log.unwrap();
      let client = ctx.get_client_with_id(chain)?;
      let cached = ctx.read(|ctx| {
         ctx.pool_manager
            .get_v3_pool_from_address(chain, pool_address)
      });

      let pool = if let Some(pool) = cached {
         pool
      } else {
         let pool = UniswapV3Pool::from_address(client, chain, pool_address).await?;
         ctx.write(|ctx| ctx.pool_manager.add_v3_pools(vec![pool.clone()]));
         ctx.write(|ctx| {
            ctx.currency_db
               .insert_currency(chain, Currency::from(pool.token0.clone()))
         });
         ctx.write(|ctx| {
            ctx.currency_db
               .insert_currency(chain, Currency::from(pool.token1.clone()))
         });
         pool
      };

      let (amount_in, token_in) = if swap_log.amount0.is_positive() {
         (swap_log.amount0, pool.token0.clone())
      } else {
         (swap_log.amount1, pool.token1.clone())
      };

      let (amount_out, token_out) = if swap_log.amount1.is_negative() {
         (swap_log.amount0, pool.token0.clone())
      } else {
         (swap_log.amount1, pool.token1.clone())
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
         bail!("Not a transfer")
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
         bail!("Call is not an approve");
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
         bail!("Failed to decode approve log");
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
