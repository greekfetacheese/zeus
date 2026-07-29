# Embedded binary assets (compile-time `include_bytes!`)

Files under this tree are baked into the **Zeus binary** at compile time.
They are **not** part of the `zeus-railgun` crate.

## Layout

```text
embedded/
  railgun/
    01x01/
      wasm.br
      proving_key.bin.br
      matrices.bin.br
    01x02/ …
```

## Railgun circuits

Source: [Robert-MacWha/privacy-protocol-artifacts](https://github.com/Robert-MacWha/privacy-protocol-artifacts/tree/refs/heads/main/artifacts/railgun)

Current hot-set (see `src/embedded/railgun.rs`):

| Circuit | Role |
|---------|------|
| `01x01` | Self-broadcast single-note unshield |
| `01x02` | Self-broadcast unshield with change |
| `01x03` | Private broadcast (+ fee outs) |
| `02x01` | Merge private notes |

To add a circuit: drop the three `.br` files into `embedded/railgun/NNxMM/`
and append `railgun_circuit!("NNxMM")` in `src/embedded/railgun.rs`.

Embedded circuits are registered on the loader as always-available offline
sources and are **not** written into the user disk cache on load/prefetch.
