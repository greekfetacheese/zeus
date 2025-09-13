#[cfg(test)]
mod tests {
   use crate::core::ZeusCtx;

   use zeus_eth::{abi::zeus::ZeusStateView, amm::uniswap::UniswapPool};
   use zeus_eth::{alloy_primitives::U256, revm_utils::*, types::*, utils::batch};

   #[tokio::test]
   async fn test_get_eth_balance() {
      let ctx = ZeusCtx::new();

      let mut good_rpcs = Vec::new();
      let mut bad_rpcs = Vec::new();

      for chain in SUPPORTED_CHAINS {
         let rpcs = ctx.rpc_providers().get_all(chain);

         for rpc in &rpcs {
            let client = match ctx.connect_to_rpc(rpc).await {
               Ok(client) => client,
               Err(e) => {
                  eprintln!(
                     "Error connecting to client using {} {}",
                     rpc.url, e
                  );
                  continue;
               }
            };

            let alice = DummyAccount::new(AccountType::EOA, U256::ZERO);
            match batch::get_eth_balances(client.clone(), chain, None, vec![alice.address]).await {
               Ok(balances) => {
                  assert_eq!(balances.len(), 1);
                  assert_eq!(balances[0].owner, alice.address);
                  assert_eq!(balances[0].balance, U256::ZERO);
                  good_rpcs.push(rpc.clone());
               }
               Err(e) => {
                  eprintln!(
                     "Error getting ETH balance using {} {}",
                     rpc.url, e
                  );
                  bad_rpcs.push(rpc.clone());
               }
            };
         }
      }

      eprintln!("Good RPCs: {}", good_rpcs.len());
      eprintln!("Bad RPCs: {}", bad_rpcs.len());

      for bad in &bad_rpcs {
         eprintln!("Bad RPC: {}", bad.url);
      }
   }

   #[tokio::test]
   async fn test_v3_pool_state() {
      let ctx = ZeusCtx::new();

      let mut good_rpcs = Vec::new();
      let mut bad_rpcs = Vec::new();

      for chain in SUPPORTED_CHAINS {
        if chain == 56 {
            continue;
        }

         let rpcs = ctx.rpc_providers().get_all(chain);
         let manager = ctx.pool_manager();
         let all_pools = manager.get_v3_pools_for_chain(chain);

         let mut pools = Vec::new();

         for pool in all_pools {
            if pools.len() == 15 {
               break;
            }
            let p = ZeusStateView::V3Pool {
               addr: pool.address(),
               tokenA: pool.currency0().address(),
               tokenB: pool.currency1().address(),
               fee: pool.fee().fee_u24(),
            };
            pools.push(p);
         }

         eprintln!("Chain {} has {} pools", chain, pools.len());

         for rpc in &rpcs {
            let client = match ctx.connect_to_rpc(rpc).await {
               Ok(client) => client,
               Err(e) => {
                  eprintln!(
                     "Error connecting to client using {} {}",
                     rpc.url, e
                  );
                  continue;
               }
            };

            match batch::get_v3_state(client.clone(), chain, pools.clone()).await {
               Ok(pool_data) => {
                  assert_eq!(pool_data.len(), pools.len());
                  good_rpcs.push(rpc.clone());
               }
               Err(e) => {
                  eprintln!(
                     "Error getting V3 pool state using {} {}",
                     rpc.url, e
                  );
                  bad_rpcs.push(rpc.clone());
               }
            };
         }
      }

      println!("Good RPCs: {}", good_rpcs.len());
      println!("Bad RPCs: {}", bad_rpcs.len());

      for bad in &bad_rpcs {
         println!("Bad RPC: {}", bad.url);
      }
   }
}
