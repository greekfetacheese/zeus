use crate::core::ZeusCtx;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use zeus_eth::{
   abi::erc20,
   alloy_primitives::{Address, Bytes, Log, U256},
   currency::{Currency, NativeCurrency},
   utils::NumericValue,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransferParams {
   pub currency: Currency,
   pub amount: NumericValue,
   /// USD value at the time of the tx
   pub amount_usd: Option<NumericValue>,
   /// Real amount sent (in case of a transfer tax)
   pub real_amount_sent: Option<NumericValue>,
   pub real_amount_sent_usd: Option<NumericValue>,
   pub sender: Address,
   pub recipient: Address,
}

impl TransferParams {
   pub fn name(&self) -> String {
      if self.currency.is_native() {
         return "Transfer".to_string();
      } else {
         return "ERC20 Transfer".to_string();
      }
   }

   pub async fn new(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      interact_to: Address,
      call_data: Bytes,
      value: U256,
      log: &Log,
   ) -> Result<Self, anyhow::Error> {
      if let Ok(native) = Self::native(
         ctx.clone(),
         chain,
         from,
         interact_to,
         call_data,
         value,
      ) {
         return Ok(native);
      }

      if let Ok(erc20) = Self::from_erc20_log(ctx.clone(), chain, log).await {
         return Ok(erc20);
      }

      Err(anyhow!("Transaction is not a transfer"))
   }

   pub async fn from_erc20_log(ctx: ZeusCtx, chain: u64, log: &Log) -> Result<Self, anyhow::Error> {
      let mut transfer_log = None;
      let mut token_address = None;

      if let Ok(decoded) = erc20::decode_transfer_log(log) {
         transfer_log = Some(decoded);
         token_address = Some(log.address);
      }

      if transfer_log.is_none() {
         return Err(anyhow!("Failed to decode transfer log"));
      }

      let transfer_log = transfer_log.unwrap();
      let token_address = token_address.unwrap();

      // A mint is not a transfer between two accounts. The ERC-20 standard's own shape for creating
      // tokens is a `Transfer` from the zero address, and taking that at face value names
      // `0x0000…0000` as the sender — a participant that does not exist, on a transaction that may
      // otherwise say nothing else (a lone mint log is ranked as the main event). The tokens arriving
      // are what the receipt's balance diff is for. The native path refuses the same sender
      // ([`Self::native_transfer`]), and this guard sits before the token lookup so a mint costs no RPC.
      if transfer_log.from.is_zero() {
         return Err(anyhow!("Transfer from zero address"));
      }

      let token = ctx.get_token(chain, token_address).await?;

      let amount = NumericValue::format_wei(transfer_log.value, token.decimals);
      let amount_usd =
         ctx.get_currency_value_for_amount(amount.f64(), &Currency::from(token.clone()));

      let currency = Currency::from(token);
      let sender = transfer_log.from;
      let recipient = transfer_log.to;

      Ok(Self {
         currency,
         amount,
         amount_usd: Some(amount_usd),
         real_amount_sent: None,
         real_amount_sent_usd: None,
         sender,
         recipient,
      })
   }

   /// A native transfer described by the transaction itself.
   pub fn native(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      to: Address,
      call_data: Bytes,
      value: U256,
   ) -> Result<Self, anyhow::Error> {
      if call_data.len() != 0 {
         return Err(anyhow!("Not a native transfer"));
      }

      Self::native_transfer(ctx, chain, from, to, value)
   }

   /// A native transfer read from an [EIP-7708](https://eips.ethereum.org/EIPS/eip-7708) log.
   ///
   /// The endpoints and the amount come from the log rather than the transaction, so this also
   /// describes an ETH transfer a contract made mid-execution — which nothing else records.
   pub fn from_eth_transfer_log(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      to: Address,
      value: U256,
   ) -> Result<Self, anyhow::Error> {
      Self::native_transfer(ctx, chain, from, to, value)
   }

   fn native_transfer(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      to: Address,
      value: U256,
   ) -> Result<Self, anyhow::Error> {
      if from.is_zero() {
         return Err(anyhow!("Transfer from zero address"));
      }

      let native: Currency = NativeCurrency::from_chain_id(chain)?.into();
      let amount = NumericValue::format_wei(value, native.decimals());
      let amount_usd = ctx.get_currency_value_for_amount(amount.f64(), &native);

      Ok(Self {
         currency: native,
         amount,
         amount_usd: Some(amount_usd),
         real_amount_sent: None,
         real_amount_sent_usd: None,
         sender: from,
         recipient: to,
      })
   }

   pub fn is_erc20_transfer(&self) -> bool {
      self.currency.is_erc20()
   }

   pub fn is_native_transfer(&self) -> bool {
      self.currency.is_native()
   }
}
