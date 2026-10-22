# zeus-core

The wallet domain that used to live in the binary's `src/core`: the `ZeusContext` and its
managers, the transaction and signing pipelines, the vault, wallet state and the `data/`
persistence catalog. It is a plain library with **no GUI code** — no `egui`, `eframe`, `rfd` or
other windowing dependency is compiled into it. User prompts (transaction confirmation, signing
requests, yes/no questions, notifications) reach the interface through the `UiHost` port in
`zeus_core::ui`: the binary installs its implementation once at startup, and a build with no host
attached refuses the prompt (reporting `PromptOpen::Unavailable`) instead of waiting for an answer
nobody can give.

## Modules

- `assets` — the `EmbeddedAssets` trait through which the host installs shipped asset bytes (pool
  snapshot, ERC-7730 registry, Railgun circuit artefacts); the crate itself embeds none.
- `clear_signing` — ERC-7730 descriptor registry and formatting for EIP-712 payloads and calldata.
- `context` — `ZeusCtx`/`ZeusContext` and its managers: RPC clients, balances, approvals, prices,
  pools, portfolio, NFTs, ENS, address book and the transaction database.
- `data_export` — packs the known `data/` files into a zip that mirrors the on-disk layout.
- `data_import` — validates and imports such a zip, including the vault and wallet-state unlock,
  before swapping it into `data/`.
- `persisted` — the catalog of file and directory names under `data/`, with export rules attached.
- `platform` — private file-system helpers (owner-only permissions, atomic writes) and time and
  display utilities.
- `signature` — message and Permit2 signing helpers, plus EIP-4361 "Sign-In With Ethereum"
  detection.
- `sim` — revm fork simulation of transactions and the resulting approval and balance diffs.
- `tx` — transaction analysis, approval, sending, confirmation, finalisation, event decoding and
  notifications.
- `types` — serde types for persisted settings and cached chain, dapp and wallet data.
- `ui` — the `UiHost` port and prompt types through which core reaches the interface.

## License

MIT OR Apache-2.0.

## Part of [Zeus](https://github.com/greekfetacheese/zeus).
