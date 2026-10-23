use alloy_primitives::U256;
use ark_bn254::Fr;
use ark_ff::PrimeField;
use secure_types::Zeroize;

pub fn poseidon_hash(inputs_u256: &[U256]) -> Result<U256, zeus_poseidon::error::Error> {
   let mut inputs: Vec<Fr> = inputs_u256
      .iter()
      .map(|i| Fr::from_be_bytes_mod_order(&i.to_be_bytes::<32>()))
      .collect();
   let hash = zeus_poseidon::poseidon_hash(&inputs)?;
   inputs.zeroize();

   Ok(hash.into_bigint().into())
}

/// Poseidon over one input. The arity is in the name, so the caller cannot get it wrong.
pub fn poseidon1(a: U256) -> U256 {
   poseidon_hash(&[a]).expect("poseidon supports 1 input")
}

/// Poseidon over two inputs.
pub fn poseidon2(a: U256, b: U256) -> U256 {
   poseidon_hash(&[a, b]).expect("poseidon supports 2 inputs")
}

/// Poseidon over three inputs.
pub fn poseidon3(a: U256, b: U256, c: U256) -> U256 {
   poseidon_hash(&[a, b, c]).expect("poseidon supports 3 inputs")
}
