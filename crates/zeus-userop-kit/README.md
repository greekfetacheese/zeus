# zeus-userop-kit

ERC-4337 user operation builder and bundler client for EntryPoint 0.7 and 0.8. It is desktop-only:
it ships the Pimlico bundler and Simple7702Account support and has no WASM or JavaScript bindings.

## What this fork changes

`zeus-userop-kit` is Zeus's fork of Ethereum's [kohaku](https://github.com/ethereum/kohaku)
`userop-kit` crate, which is MIT-licensed. Zeus dropped the `dyn` trait and the WASM/JS bindings and
added the Pimlico bundler path and Simple7702Account support. The upstream Rust implementations were
later deprecated and removed from kohaku, so this fork is the maintained line.

## Installation

```bash
cargo add zeus-userop-kit
```

## Modules

- `user_operation`: the unpacked JSON-RPC wire types — `UserOperation`, EIP-7702 `Authorization`,
  operation hashes, gas estimates and receipts — for EntryPoint 0.7 and 0.8.
- `signable_user_operation`: an unsigned operation bound to its EntryPoint, with its EIP-712 hash.
- `signed_user_operation`: a signed operation with an optional signed 7702 authorization, ready to
  submit to a bundler.
- `entry_point`: the EntryPoint v0.8 address and its EIP-712 domain.
- `abis`: the EntryPoint contract ABI bindings.
- `builder`: `UserOperationBuilder`, which assembles an operation with nonce, calldata, gas,
  paymaster, factory and authorization fields.
- `bundler`: the Pimlico bundler client (gas estimation, submission, receipt polling), its JSON-RPC
  transport and `BundlerError`.
- `smart_account`: Simple7702Account support — address, nonce, authorization and calldata encoding.

## License

MIT. See `LICENCE`.
