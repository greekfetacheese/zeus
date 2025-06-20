#[cfg(test)]
mod tests {
   use std::str::FromStr;

   use crate::core::ZeusCtx;

   use zeus_eth::{
      alloy_primitives::{Address, Bytes, TxKind},
      alloy_provider::{Provider, ProviderBuilder},
      alloy_rpc_types::BlockId,
      alloy_sol_types::SolValue,
      amm::UniswapPool,
      revm_utils::{revm::bytecode::Bytecode, *},
      utils::batch,
   };

   const GET_V3_POOL_STATE_BYTECODE: &str = "0x6080806040526004361015610012575f80fd5b5f3560e01c63c3401e0714610025575f80fd5b346102dc5760207ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffc3601126102dc57600435905f548210156102dc5760ff600c6101e0935f80520273ffffffffffffffffffffffffffffffffffffffff817f290decd9548b62a8d60345a988386fc84ba6bc95484008f6362f93160ef3e563015416907f290decd9548b62a8d60345a988386fc84ba6bc95484008f6362f93160ef3e5648101547f290decd9548b62a8d60345a988386fc84ba6bc95484008f6362f93160ef3e5658201547f290decd9548b62a8d60345a988386fc84ba6bc95484008f6362f93160ef3e5668301547f290decd9548b62a8d60345a988386fc84ba6bc95484008f6362f93160ef3e5678401547f290decd9548b62a8d60345a988386fc84ba6bc95484008f6362f93160ef3e5688501547f290decd9548b62a8d60345a988386fc84ba6bc95484008f6362f93160ef3e569860154916fffffffffffffffffffffffffffffffff7f290decd9548b62a8d60345a988386fc84ba6bc95484008f6362f93160ef3e56a88015416937f290decd9548b62a8d60345a988386fc84ba6bc95484008f6362f93160ef3e56b880154957f290decd9548b62a8d60345a988386fc84ba6bc95484008f6362f93160ef3e56c890154977f290decd9548b62a8d60345a988386fc84ba6bc95484008f6362f93160ef3e56e7f290decd9548b62a8d60345a988386fc84ba6bc95484008f6362f93160ef3e56d8b01549a01549a8d5260208d015260408c015260608b015260808a015260a089015260c088015260e087015273ffffffffffffffffffffffffffffffffffffffff811661010087015260a01c60020b6101208601526101408501528060010b61016085015260101c600f0b6101808401526fffffffffffffffffffffffffffffffff81166101a084015260801c1615156101c0820152f35b5f80fdfea264697066735822122004fe465b6dc52c0aea9ad8b5dd4ced2f6a7137bcb85242562481f4f427f91d0064736f6c634300081c0033";

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_get_v3_batch_state() {
      let url = "https://reth-ethereum.ithaca.xyz/rpc".parse().unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 1;

      let ctx = ZeusCtx::new();

      let pool_manager = ctx.pool_manager();
      let v3_pools = pool_manager.get_v3_pools_for_chain(chain_id);

      if v3_pools.len() < 10 {
         panic!("Cannot continue test, less than 10 v3 pools found");
      }

      let mut pools_to_update = Vec::new();
      for pool in &v3_pools {
         if pools_to_update.len() >= 10 {
            break;
         }
         pools_to_update.push(batch::V3Pool {
            pool: pool.address(),
            token0: pool.currency0().address(),
            token1: pool.currency1().address(),
            tickSpacing: pool.fee().tick_spacing(),
         });
      }

      let res = batch::get_v3_state(client, None, pools_to_update)
         .await
         .unwrap();

      if res.len() < 10 {
         panic!(
            "Requested the state for 10 pools but got back only for {}",
            res.len()
         );
      }
   }
}
