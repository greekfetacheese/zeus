# <p align="center">zeus-tokens</p>

## Standalone CLI that generates Zeus default token-list blobs.

## Part of [Zeus](https://github.com/greekfetacheese/zeus).

Compiles **without** the Zeus GUI. It sparse-clones [Trust Wallet assets](https://github.com/trustwallet/assets) at runtime and runs the same pipeline as the original util:

1. **remove garbage** — drop `status: abandoned` and `status: spam`
2. **resize icons** — write each `logo.png` as 32×32 (Lanczos3)
3. **make token data** — keep liquid / base tokens, pack 32px + 24px PNG icons, write bincode

Output is the same `token_data.data` blob Zeus already embeds:

```
embedded/token_data.data
```

---

## Build

From the Zeus repo root:

```bash
cargo build -p zeus-tokens
```

Release:

```bash
cargo build -p zeus-tokens --release
```

Binary: `target/debug/zeus-tokens` (or `target/release/zeus-tokens`).

```bash
./target/debug/zeus-tokens --help
```

---

## Usage

```text
zeus-tokens [OPTIONS]

      --chain <CHAIN>              Repeatable. 1 / ethereum, 10 / optimism, 56 / binance,
                                   8453 / base, 42161 / arbitrum
                                   [default: all five]
      --work-dir <WORK_DIR>        Trust Wallet sparse clone / asset tree   [default: token_data]
      --out <OUT>                  Output blob                              [default: embedded/token_data.data]
      --skip-download              Use `--work-dir` as-is (no git clone)
      --force-download             Delete `--work-dir` and clone again
      --repo <REPO>                Git URL                                  [default: https://github.com/trustwallet/assets.git]
      --git-ref <GIT_REF>          Branch / ref                             [default: master]
      --rpc-ethereum <URL>         [env: ETHEREUM_RPC]
      --rpc-optimism <URL>         [env: OPTIMISM_RPC]
      --rpc-binance <URL>          [env: BINANCE_RPC]
      --rpc-base <URL>             [env: BASE_RPC]
      --rpc-arbitrum <URL>         [env: ARBITRUM_RPC]
      --concurrency <N>            Concurrent liquidity checks              [default: 16]
```

`--rpc-*` is required for each **non-BSC** chain you encode. BSC is still downloaded / garbage-filtered / resized (Trust Wallet folder `binance`) but is not written into the blob — same as the original util.

`git` must be on `PATH` unless you pass `--skip-download`.

Default `--work-dir` and `--out` are relative to the **current working directory**. Zeus embeds `embedded/token_data.data`, so run this from the Zeus folder if you want the next wallet build to pick the file up.

---

## Examples

### Full list (download + filter + encode)

Needs archive-capable RPCs. Public endpoints often rate-limit the Uniswap pool probes.

```bash
export ETHEREUM_RPC=...
export OPTIMISM_RPC=...
export BASE_RPC=...
export ARBITRUM_RPC=...

./zeus-tokens --out embedded/token_data.data
```

### One chain

```bash
./zeus-tokens \
  --chain ethereum \
  --rpc-ethereum "$ETHEREUM_RPC" \
  --out /tmp/token_data.data
```

### Re-run encoding on an existing tree

```bash
./zeus-tokens \
  --skip-download \
  --work-dir token_data \
  --rpc-ethereum "$ETHEREUM_RPC" \
  --rpc-optimism "$OPTIMISM_RPC" \
  --rpc-base "$BASE_RPC" \
  --rpc-arbitrum "$ARBITRUM_RPC"
```

`--work-dir` accepts either a sparse clone (`blockchains/ethereum/assets/...`) or the old flat layout (`ethereum/assets/...`).

---

## Output

On success:

```
embedded/token_data.data
```

Rebuild Zeus afterwards so `include_bytes!` picks up the new blob.

---

## Notes

- Liquidity gate is unchanged: a token is kept if it is a base token **or** it has a Uniswap V2/V3/V4 pool whose base-side USD value is ≥ `$10,000`.
- Deleting `--work-dir` (or `--force-download`) forces a full re-fetch.
- Logs: `RUST_LOG=info,zeus_eth=debug`.
- Chains: Ethereum, Optimism, BNB Smart Chain (assets only), Base, Arbitrum.
