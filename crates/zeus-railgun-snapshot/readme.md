# <p align="center">zeus-railgun-snapshot</p>

## Standalone CLI that generates Railgun `events-snapshot` blobs for Zeus.

## Part of [Zeus](https://github.com/greekfetacheese/zeus).

Compiles **without** the Zeus GUI. It reuses `RpcSyncer` / `SubsquidSyncer` + `SnapshotLoader` from `zeus-railgun` and writes the same files the wallet already loads:

```
events-snapshot:{chain}.data
events-snapshot:{chain}.meta
```

Resume-safe: if a blob already exists in `--out`, the syncer extends it from the covered tip instead of starting over.

---

## Build

From the Zeus repo root:

```bash
cargo build -p zeus-railgun-snapshot
```

Release:

```bash
cargo build -p zeus-railgun-snapshot --release
```

Binary: `target/debug/railgun-snapshot` (or `target/release/railgun-snapshot`).

```bash
./target/debug/railgun-snapshot --help
```

---

## Usage

```text
railgun-snapshot [OPTIONS]

      --chain <CHAIN>              1 / mainnet / eth  or  11155111 / sepolia   [default: 1]
      --rpc <RPC>                  HTTP JSON-RPC URL  [env: ETH_RPC_URL]
      --out <OUT>                  Output directory                             [default: data/railgun]
      --source <SOURCE>            rpc | subsquid                               [default: rpc]
      --from <FROM>                Inclusive start block (default: Railgun deployment)
      --to <TO>                    Inclusive end block (default: chain tip)
      --block-range <BLOCK_RANGE>  eth_getLogs chunk size (RPC only)
      --concurrency <CONCURRENCY>  Concurrent eth_getLogs chunks (RPC only)
```

`--rpc` is required for `--source rpc`. For Subsquid it is only needed if you omit `--to` and want the RPC tip (Subsquid can resolve the tip itself).

Default `--out` is relative to the **current working directory**. Zeus loads snapshots from `data/railgun` next to the wallet binary, so run this from the Zeus folder (or pass that path) if you want the app to pick the files up.

---

## Examples

### Mainnet via archive RPC (deployment → tip)

Needs an archive node. Public RPCs usually drop or silently truncate `eth_getLogs` over large ranges.

```bash
./railgun-snapshot \
  --chain mainnet \
  --rpc "$ETH_RPC_URL" \
  --out data/railgun
```

### Mainnet via Subsquid (faster historical pull)

```bash
./railgun-snapshot \
  --chain mainnet \
  --source subsquid \
  --out data/railgun
```

### Bounded range

```bash
./railgun-snapshot \
  --chain mainnet \
  --source rpc \
  --rpc "$ETH_RPC_URL" \
  --from 5784774 \
  --to 5800000 \
  --out /tmp/railgun-snap
```

`--from` defaults to the Railgun smart-wallet deployment block (`14693013` mainnet, `5784774` Sepolia). Do not start above that if you want a blob Zeus can use for a full historical resync.

### Paid RPC: larger chunks / more concurrency

Defaults are conservative (`3000` blocks / `2` concurrent on mainnet, `30000` on Sepolia) because many nodes return **partial** logs instead of an error when the range is too wide.

```bash
./railgun-snapshot \
  --chain mainnet \
  --rpc "$ETH_RPC_URL" \
  --block-range 10000 \
  --concurrency 4 \
  --out data/railgun
```

---

## Output

On success you get something like:

```
data/railgun/events-snapshot:1.data
data/railgun/events-snapshot:1.meta
```

Sepolia uses chain id `11155111` in the filename.

Copy those two files into the Zeus `data/railgun/` directory (or generate them there with `--out data/railgun`). Zeus will replay the blob on a new-signer / historical sync and only RPC the tail after `block_number`.

---

## Notes

- This tool does **not** write the Railgun redb (`railgun:{chain}.db`). It only builds the events cache.
- Deleting the `.data` / `.meta` pair forces a full re-fetch on the next run.
- Logs: `RUST_LOG=info,zeus_railgun=debug` (default filter is already close to that).
- Supported chains today: Ethereum mainnet and Sepolia (`ChainConfig` in `zeus-railgun`).
