use alloy_rpc_types::Block;

/// Calculate the next block base fee
// based on math provided here: https://ethereum.stackexchange.com/questions/107173/how-is-the-base-fee-per-gas-computed-for-a-new-block
pub fn calculate_next_block_base_fee(block: Block) -> u64 {
   // Get the block base fee per gas
   let current_base_fee_per_gas = block.header.base_fee_per_gas.unwrap_or_default();

   // Get the mount of gas used in the block
   let current_gas_used = block.header.gas_used;

   let current_gas_target = block.header.gas_limit / 2;

   if current_gas_used == current_gas_target {
      current_base_fee_per_gas
   } else if current_gas_used > current_gas_target {
      let gas_used_delta = current_gas_used - current_gas_target;
      let base_fee_per_gas_delta = current_base_fee_per_gas * gas_used_delta / current_gas_target / 8;

      return current_base_fee_per_gas + base_fee_per_gas_delta;
   } else {
      let gas_used_delta = current_gas_target - current_gas_used;
      let base_fee_per_gas_delta = current_base_fee_per_gas * gas_used_delta / current_gas_target / 8;

      return current_base_fee_per_gas - base_fee_per_gas_delta;
   }
}
