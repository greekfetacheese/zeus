# Circom Compatible Poseidon Hash

> This crate was forked from [taceolabs/poseidon-rust](https://github.com/TaceoLabs/poseidon-rust) under the MIT license. It is published on crates.io as [`zeus-poseidon`](https://crates.io/crates/zeus-poseidon). The original README is available below.
>
> This crate has been modified to:
>  - Add a `poseidon_hash` function to hash an arbitrary number of field elements
>  - Add benchmarks for the `poseidon_hash` function

This crate contains a custom rust implementation of the Poseidon hash function, which is compatible with [Circom](https://docs.circom.io/).
Currently, only the parameters for Poseidon with statesizes t=3 (allows hashing two field elements) and t=4 (allows hashing three field elements) for the BN254 curve is present. However, different parameters can be generated with the `src/bn254/parameters.sage` script, which is a modified copy of the original Poseidon instance generation script from [https://extgit.iaik.tugraz.at/krypto/hadeshash/-/blob/master/code/generate_parameters_grain.sage](https://extgit.iaik.tugraz.at/krypto/hadeshash/-/blob/master/code/generate_parameters_grain.sage).

Usage (for the BN254 curve):

```sage
sage parameters.sage 1 0 254 <t> 8 <r> 0x30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000001
```

Or generate the whole thing with:

```bash
nix-shell -p sage
just generate
```

where t is the statesize and r is the number of rounds. Circom uses the following round numbers for different statesizes t:

| t   | r   |
| --- | --- |
| 2   | 56  |
| 3   | 57  |
| 4   | 56  |
| 5   | 60  |
| 6   | 60  |
| 7   | 63  |
| 8   | 64  |
| 9   | 63  |
| 10  | 60  |
| 11  | 66  |
| 12  | 60  |
| 13  | 65  |
| 14  | 70  |
| 15  | 60  |
| 16  | 64  |
| 17  | 68  |

## Usage

```rust
use ark_bn254::Fr;

// hashes 1..=13 field elements (circom statesizes t=2..=14)
let hash = zeus_poseidon::poseidon_hash(&[Fr::from(1u64), Fr::from(2u64)])?;
```

## License

MIT. Copyright (c) 2024 TaceoLabs for the original work; the `poseidon_hash` addition and its
benchmarks are part of Zeus. See `LICENCE`.
