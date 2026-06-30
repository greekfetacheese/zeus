# zeus-railgun-prover

Node.js sidecar + Rust client for generating **real** Railgun Groth16 proofs.

## Architecture (same as zeus-waku-broadcaster)

- **JS sidecar** (`js-sidecar/`): Dumb pipe.
  - Downloads proving artifacts (zkey, wasm, vkey) from Railgun's official IPFS.
  - Runs `snarkjs.groth16.fullProve`.
  - Returns `SnarkProof` in the exact format the Railgun contracts expect.

- **Rust crate**: Owns witness construction from our `PreparedUnshield`, `RailgunScanner`, and `RailgunKeys`.

## Usage (future)

```rust
let prover = RailgunProverClient::start("./crates/zeus-railgun-prover/js-sidecar").await?;

let proof = prover.prove(witness_json, "01x02").await?;
```

## Next steps

- Map our internal `PreparedUnshield` + merkle path data into the exact `FormattedCircuitInputsRailgun` shape expected by the circuit (see engine/src/prover/prover.ts `formatRailgunInputs`).
- Wire this into `build_unshield_transact_calldata` (replace dummy proof).
- Support native-prover as an option for speed (like the official wallets do).

This is the pragmatic path to on-chain valid private transactions.
