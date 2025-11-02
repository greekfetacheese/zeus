#[cfg(test)]
mod tests {
   use crate::core::ZeusCtx;

   use zeus_eth::utils::address_book::{uniswap_v4_stateview, zeus_stateview_v2};
   use zeus_eth::{abi::zeus::ZeusStateViewV2, amm::uniswap::UniswapPool};
   use zeus_eth::{
      alloy_primitives::{TxKind, U256},
      alloy_sol_types::SolCall,
      revm_utils::*,
      types::*,
      utils::batch,
   };

   #[tokio::test]
   async fn test_get_eth_balance() {
      let ctx = ZeusCtx::new();

      let mut good_rpcs = Vec::new();
      let mut bad_rpcs = Vec::new();

      for chain in SUPPORTED_CHAINS {
         let z_client = ctx.get_zeus_client();
         let rpcs = z_client.get_rpcs(chain);

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

         let z_client = ctx.get_zeus_client();
         let rpcs = z_client.get_rpcs(chain);
         let manager = ctx.pool_manager();
         let all_pools = manager.get_v3_pools_for_chain(chain);

         let mut pools = Vec::new();

         for pool in all_pools {
            if pools.len() == 20 {
               break;
            }
            let p = ZeusStateViewV2::V3Pool {
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

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn test_v3_state_gas_used() {
      let ctx = ZeusCtx::new();

      let chain = 1;
      let contract_address = zeus_stateview_v2(chain).unwrap();
      let pool_manager = ctx.pool_manager();
      let all_pools = pool_manager.get_v3_pools_for_chain(chain);

      let mut pools = Vec::new();

      for pool in all_pools {
         if pools.len() == 20 {
            break;
         }
         let p = ZeusStateViewV2::V3Pool {
            addr: pool.address(),
            tokenA: pool.currency0().address(),
            tokenB: pool.currency1().address(),
            fee: pool.fee().fee_u24(),
         };
         pools.push(p);
      }

      let client = ctx.get_client(chain).await.unwrap();
      let fork_factory = ForkFactory::new_sandbox_factory(client.clone(), chain, None, None);
      let fork_db = fork_factory.new_sandbox_fork();
      let mut evm = new_evm(chain.into(), None, fork_db);

      let data = ZeusStateViewV2::getV3PoolStateCall {
         pools: pools.clone(),
      }
      .abi_encode();

      evm.tx.data = data.into();
      evm.tx.kind = TxKind::Call(contract_address);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      println!("Gas used: {}", res.gas_used());
      println!("Success: {}", res.is_success());
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn test_v4_state_gas_used() {
      let ctx = ZeusCtx::new();

      let chain = 1;
      let contract_address = zeus_stateview_v2(chain).unwrap();
      let uni_stateview = uniswap_v4_stateview(chain).unwrap();
      let pool_manager = ctx.pool_manager();
      let all_pools = pool_manager.get_v4_pools_for_chain(chain);

      let mut pools = Vec::new();

      for pool in all_pools {
         if pools.len() == 20 {
            break;
         }

         let p = ZeusStateViewV2::V4Pool {
            pool: pool.id(),
            tickSpacing: pool.fee().tick_spacing(),
         };

         pools.push(p);
      }

      let client = ctx.get_client(chain).await.unwrap();
      let fork_factory = ForkFactory::new_sandbox_factory(client.clone(), chain, None, None);
      let fork_db = fork_factory.new_sandbox_fork();
      let mut evm = new_evm(chain.into(), None, fork_db);

      let data = ZeusStateViewV2::getV4PoolStateCall {
         pools: pools,
         stateView: uni_stateview,
      }
      .abi_encode();

      evm.tx.data = data.into();
      evm.tx.kind = TxKind::Call(contract_address);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      println!("Gas used: {}", res.gas_used());
      println!("Success: {}", res.is_success());
   }
}
