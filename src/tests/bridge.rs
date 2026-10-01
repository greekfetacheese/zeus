#[cfg(test)]
mod tests {
   use crate::core::{BridgeParams, types::Dapp};
   use crate::tests::test_ctx;
   use crate::utils::simulate::{pinned_head, simulate_for_analysis};

   use zeus_eth::{
      abi::protocols::across::{DepositV3Args, encode_deposit_v3},
      alloy_primitives::{Address, Bytes, U256, address},
      alloy_rpc_types::BlockId,
      currency::Currency,
      types::ChainId,
      utils::{NumericValue, address_book},
   };

   const ETHEREUM: u64 = 1;
   const OPTIMISM: u64 = 10;
   const BASE: u64 = 8453;
   const ARBITRUM: u64 = 42161;
   const ROBINHOOD: u64 = 4663;

   /// Deposits pay the fee on the way out, so the output is smaller than the input. The SpokePool
   /// does not validate the ratio (a relayer decides what it will fill); these tests only prove our
   /// half of the bridge, so the exact fee does not matter.
   const OUTPUT_FEE_PERCENT: u64 = 1;

   /// Simulate one Across deposit and return the decoded bridge parameters.
   ///
   /// A bridge cannot be simulated end to end — the fill happens on another chain, by a relayer that
   /// does not exist in this fork. What *can* be proven is our interaction with the protocol: the
   /// SpokePool call succeeds, and the `FundsDeposited` event it emits still decodes through
   /// `BridgeParams` — the same decode the confirmation window and the tx history are built from.
   /// A silent change to that event (or to the deposit ABI) is what this catches.
   async fn run_deposit(
      origin: u64,
      destination: u64,
      amount_eth: &str,
   ) -> Result<BridgeParams, anyhow::Error> {
      let ctx = test_ctx(origin);
      let chain = ChainId::from(origin);

      let amount = NumericValue::parse_to_wei(amount_eth, 18);
      let output_amount =
         amount.wei() - (amount.wei() * U256::from(OUTPUT_FEE_PERCENT) / U256::from(100));

      let depositor = address!("1111111111111111111111111111111111111111");
      let recipient = address!("2222222222222222222222222222222222222222");

      // Quote timestamps are compared against the block the deposit lands in, so pin the head and
      // quote from that block's clock.
      let (block, _) = pinned_head(ctx.clone(), chain, BlockId::latest()).await?;
      let quote_timestamp = (block.header.timestamp as u32).saturating_sub(60);
      let fill_deadline = quote_timestamp + 300;

      let call_data = encode_deposit_v3(DepositV3Args {
         depositor,
         recipient,
         // Bridging ETH means depositing the wrapped native: `depositV3` is payable and the SpokePool
         // wraps `msg.value` when the input token is its `wrappedNativeToken()`.
         input_token: Currency::wrapped_native(origin).address(),
         output_token: Currency::wrapped_native(destination).address(),
         input_amount: amount.wei(),
         output_amount,
         destination_chain_id: destination,
         exclusive_relayer: Address::ZERO,
         quote_timestamp,
         fill_deadline,
         exclusivity_deadline: 0,
         message: Bytes::default(),
      });

      let spoke_pool = address_book::across_spoke_pool_v2(origin)?;

      let sim = simulate_for_analysis(
         ctx.clone(),
         chain,
         depositor,
         spoke_pool,
         call_data,
         amount.wei(),
         Vec::new(),
      )
      .await?;

      eprintln!(
         "{} -> {} deposit of {} ETH: {} logs, {} gas",
         chain.name(),
         ChainId::from(destination).name(),
         amount.abbreviated(),
         sim.logs.len(),
         sim.gas_used
      );

      let mut params = None;
      for log in &sim.logs {
         if let Ok(decoded) = BridgeParams::from_log(ctx.clone(), origin, log).await {
            params = Some(decoded);
            break;
         }
      }

      let params = params.ok_or_else(|| {
         anyhow::anyhow!(
            "no decodable FundsDeposited log among {} logs",
            sim.logs.len()
         )
      })?;

      assert!(matches!(params.dapp, Dapp::Across));

      Ok(params)
   }

   /// Every chain Zeus can bridge from, asserted down to the decoded event.
   async fn assert_bridge(origin: u64, destination: u64) {
      let amount = "0.01";
      let params = run_deposit(origin, destination, amount)
         .await
         .unwrap_or_else(|e| panic!("{} -> {}: {e}", origin, destination));

      assert_eq!(params.origin_chain, origin);
      assert_eq!(params.destination_chain, destination);
      assert_eq!(
         params.amount.wei(),
         NumericValue::parse_to_wei(amount, 18).wei()
      );
      assert!(
         params.received.wei() < params.amount.wei(),
         "the fee comes out of the output"
      );
      assert_eq!(
         params.depositor,
         address!("1111111111111111111111111111111111111111")
      );
      assert_eq!(
         params.recipient,
         address!("2222222222222222222222222222222222222222")
      );

      // Both sides are the wrapped native, so the decoded currencies read as ETH.
      assert!(params.input_currency.is_native());
      assert!(params.output_currency.is_native());
   }

   // --------------------------------------------------------------------------------- origin chains
   //
   // The deposit only ever happens on the origin chain, so every supported chain has to be covered as
   // an origin; destinations vary to cover the pairs the UI offers, including the newest chain.

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn ethereum_eth_to_base() {
      assert_bridge(ETHEREUM, BASE).await;
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn optimism_eth_to_arbitrum() {
      assert_bridge(OPTIMISM, ARBITRUM).await;
   }

   /// The pair that failed in the field: ETH on Base to ETH on Robinhood, where the simulation
   /// returned logs that the bridge UI could not decode.
   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn base_eth_to_robinhood() {
      assert_bridge(BASE, ROBINHOOD).await;
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn arbitrum_eth_to_optimism() {
      assert_bridge(ARBITRUM, OPTIMISM).await;
   }

   /// Robinhood as the origin, which has not been tried in the app yet.
   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn robinhood_eth_to_ethereum() {
      assert_bridge(ROBINHOOD, ETHEREUM).await;
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn ethereum_eth_to_robinhood() {
      assert_bridge(ETHEREUM, ROBINHOOD).await;
   }
}
