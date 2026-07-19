use alloy_primitives::{Address, B256, aliases::U120, b256};
use alloy_sol_types::{SolValue, sol};

use crate::{
   abi::railgun::Transaction,
   account::address::RailgunAddress,
   crypto::keys::{MasterPublicKey, ViewingPublicKey},
   types::Chain,
};

/// Railgun paymaster master public key.
pub const PAYMASTER_MASTER_PUBLIC_KEY: B256 =
   b256!("0x19acdde26147205d58fd7768be7c011f08a147ef86e6b70968d09c81cef74b13");

/// Railgun paymaster viewing public key.
pub const PAYMASTER_VIEWING_PUBLIC_KEY: B256 =
   b256!("0x63ec4d326fc49c1c71064c982fb0bcbca2ba593b44ff7e8c7e4e75b401ae1d9c");

sol!(
    struct PaymasterData {
        address adapter;
        bytes adapterData;
    }

    contract RailgunFeeAdapter {
        struct AdapterData {
            bytes16 random;
            address asset;
            uint120 value;
            Transaction[] transactions;
        }
    }
);

pub fn encode_paymaster_data(adapter: Address, adapter_data: Vec<u8>) -> Vec<u8> {
   let data = PaymasterData {
      adapter,
      adapterData: adapter_data.into(),
   };
   data.abi_encode()
}

pub fn encode_railgun_adapter_data(
   random: [u8; 16],
   asset: Address,
   value: u128,
   transactions: Vec<Transaction>,
) -> Vec<u8> {
   let data = RailgunFeeAdapter::AdapterData {
      random: random.into(),
      asset,
      value: U120::saturating_from(value),
      transactions,
   };
   data.abi_encode()
}

/// Decode `(fee_asset, fee_value_wei)` from UserOperation.paymaster_data
/// (`PaymasterData { adapter, adapterData: AdapterData { random, asset, value, txs } }`).
///
/// This is the private broadcaster/paymaster fee note amount (wrapped base token),
/// not the protocol Unshield event fee.
pub fn decode_fee_from_paymaster_data(data: &[u8]) -> Result<(Address, u128), anyhow::Error> {
   let pm =
      PaymasterData::abi_decode(data).map_err(|e| anyhow::anyhow!("decode PaymasterData: {e}"))?;
   let adapter = RailgunFeeAdapter::AdapterData::abi_decode(pm.adapterData.as_ref())
      .map_err(|e| anyhow::anyhow!("decode RailgunFeeAdapter::AdapterData: {e}"))?;

   // U120 -> u128 (fee values fit comfortably)
   let value = u128::try_from(adapter.value)
      .map_err(|_| anyhow::anyhow!("fee value does not fit in u128"))?;
   Ok((adapter.asset, value))
}

pub fn paymaster_railgun_address(_chain_id: Chain) -> RailgunAddress {
   RailgunAddress::from_public_keys(
      MasterPublicKey::from_bytes(*PAYMASTER_MASTER_PUBLIC_KEY),
      ViewingPublicKey::from_bytes(*PAYMASTER_VIEWING_PUBLIC_KEY),
      None,
   )
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_primitives::address;

   #[test]
   fn fee_encode_decode_roundtrip() {
      let asset = address!("0xfff9976782d46cc05630d1f6ebab18b2324d6b14");
      let value = 3_931_601_980_527_289u128;
      let adapter = address!("0xeBabF510f824a349a9Be7F40cad3486B7249b1e0");
      let random = [7u8; 16];

      let adapter_data = encode_railgun_adapter_data(random, asset, value, Vec::new());
      let pm_data = encode_paymaster_data(adapter, adapter_data);

      let (decoded_asset, decoded_value) = decode_fee_from_paymaster_data(&pm_data).unwrap();
      assert_eq!(decoded_asset, asset);
      assert_eq!(decoded_value, value);
   }
}
