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

## Recent Improvements (2026-06-30)

- **Disk caching** of artifacts in `~/.railgun/artifacts-v2.1`
- **Progress reporting** from the sidecar (`progress` events)
- **Optional native prover** support (install `@railgun-privacy/native-prover` in js-sidecar for speed)
- Proper Rust types for `FormattedCircuitInputsRailgun` matching the official engine
- Integration test exercising the sidecar

To prepare the sidecar:
```bash
cd crates/zeus-railgun-prover/js-sidecar
npm install
```

To run the tests:
```bash
cargo test -p zeus-railgun-prover --test sidecar_test
```


## Making the sidecar integration meaningful (done)

The Rust client now has a proper event loop in `prove()`:
- Waits for progress events (and prints them)
- Blocks until it receives either a successful `proof_generated` or a real error from the sidecar
- Returns proper `Err` on prover failure or timeout (no more silent dummy proofs)

The integration test now:
- Panics early if the sidecar can't even start
- Asserts that we received a response that came from the actual Groth16 prover (for dummy data we expect "Assert Failed")
- Clearly documents the expected behavior on first run (long download) vs cached runs

Example successful smoke test output (with cached artifacts):
```
[prover-sidecar] Artifacts ready for 01x01. Starting Groth16 proof generation...
[rust] Prover progress: proving 0%
...
[rust] Received error from sidecar: Error: Assert Failed.
[test] Confirmed: sidecar executed the Groth16 prover (dummy witness correctly rejected).
```

