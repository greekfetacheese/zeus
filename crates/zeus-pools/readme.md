# <p align="center">zeus-pools</p>

## Standalone CLI that generates Zeus default Uniswap pool blobs.

## Part of [Zeus](https://github.com/greekfetacheese/zeus).

Compiles **without** the Zeus GUI. It syncs Uniswap from factory / PoolManager deployment to tip via `zeus_eth::amm::uniswap::sync`. **Default dex is Uniswap V4** (`--dex v4`). Repeat `--dex` for V2/V3. Then:

1. **save snapshot** — unfiltered pools + checkpoints per chain (`pool_snapshots/pools:{chain}.json`). Gitignored. **Never deleted** by this tool so the next run can resume from the checkpoint. Other dexes already in the file are left alone.
2. **filter** — only the selected dex(es); same $10,000 base-side USD gate as `zeus-tokens`
3. **encode** — `PoolManager` JSON matching `zeus::core::context::pool_manager::PoolManager` (selected dexes only)

Output is the blob Zeus already embeds:

```
embedded/pool_data.json
```

Uniswap only (no Pancake). Needs an archive-capable RPC: `eth_getLogs` from each dex creation block.

---

## Build

From the Zeus repo root:

```bash
cargo build -p zeus-pools
```

Release:

```bash
cargo build -p zeus-pools --release
```

Binary: `target/debug/zeus-pools` (or `target/release/zeus-pools`).

```bash
./target/debug/zeus-pools --help
```

---

## Usage

```text
zeus-pools [OPTIONS]

      --chain <CHAIN>              Repeatable. 1 / ethereum, 10 / optimism, 56 / binance,
                                   8453 / base, 42161 / arbitrum
                                   [default: all five]
      --dex <DEX>                  Repeatable. v2 / v3 / v4 (or uniswap-v2 …)
                                   [default: v4]
      --base-tokens-only <BOOL>    Keep only pools with WETH/USDC/USDT/DAI/WBTC/LINK
                                   [default: true]
      --snapshot-dir <DIR>         Unfiltered snapshots                     [default: pool_snapshots]
      --out <OUT>                  Output JSON                              [default: embedded/pool_data.json]
      --skip-sync                  Filter + encode existing snapshots only
      --rpc-ethereum <URL>         [env: ETHEREUM_RPC]
      --rpc-optimism <URL>         [env: OPTIMISM_RPC]
      --rpc-binance <URL>          [env: BINANCE_RPC]
      --rpc-base <URL>             [env: BASE_RPC]
      --rpc-arbitrum <URL>         [env: ARBITRUM_RPC]
      --concurrency <N>            Concurrent log / state chunks            [default: 4]
      --batch-size <N>             Pools per log batch                      [default: 30]
      --block-range <N>            eth_getLogs chunk                        [default: 5000]
      --state-batch-size <N>       batch_update_state size                  [default: 20]
```

`--rpc-*` is required for every chain you encode.

Default `--snapshot-dir` and `--out` are relative to the **current working directory**. Zeus embeds `embedded/pool_data.json`, so run this from the Zeus folder if you want the next wallet build to pick the file up.

---

## Examples

### Full list (sync + filter + encode)

Needs archive RPCs. Public endpoints usually cannot scan Uniswap V2 from 2018.

```bash
export ETHEREUM_RPC=...
export OPTIMISM_RPC=...
export BINANCE_RPC=...
export BASE_RPC=...
export ARBITRUM_RPC=...

# V4 only (default)
./zeus-pools --out embedded/pool_data.json

# All Uniswap versions
./zeus-pools --dex v2 --dex v3 --dex v4 --out embedded/pool_data.json
```

### One chain

```bash
./zeus-pools \
  --chain ethereum \
  --rpc-ethereum "$ETHEREUM_RPC" \
  --out /tmp/pool_data.json
```

### Resume / re-filter

Snapshots are kept. A second run only pulls logs after each dex checkpoint. To re-apply the liquidity filter without touching the chain:

```bash
./zeus-pools --skip-sync --out embedded/pool_data.json
```

To start a chain over, **manually** delete `pool_snapshots/pools:{chain}.json`. This CLI will not remove it.

---

## Output

On success:

```
pool_snapshots/pools:1.json
pool_snapshots/pools:10.json
…
embedded/pool_data.json
```

Rebuild Zeus afterwards so `include_str!` picks up the new blob.

The snapshot is the full historical set (illiquid pools included). The embedded JSON is the filtered `PoolManager` Zeus loads on first run.

---

## Notes

- Before the liquidity RPC: drop fee **> 2%** (always) and, by default, drop pools that do not have both a base token (`ERC20Token::base_tokens()` + `wbtc()` + `link()`, plus native ETH/BNB). Disable with `--base-tokens-only false`.
- Liquidity gate is unchanged: keep a pool if its base token is native / wrapped / stable **and** base-side USD ≥ `$10,000`.
- Checkpoints in the encoded blob are copied from the **unfiltered** snapshot so Zeus does not re-scan from factory deployment.
- Logs: `RUST_LOG=info,zeus_eth=debug`.
- Chains: Ethereum, Optimism, BNB Smart Chain, Base, Arbitrum. Default dex is Uniswap V4; V2/V3 need `--dex v2` / `--dex v3`. A chain with no requested dex is skipped (e.g. BSC has no Uniswap V4).
