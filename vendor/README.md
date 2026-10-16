# Vendored dependencies

## `enigo/` — interim patch: remove when upstream ships the fix

`vendor/enigo` is the published **enigo 0.6.1** (crates.io) with a single line
changed in its manifest: the Windows dependency requirement moves from
`windows = "0.61"` to `windows = "0.62"`. The features requested are unchanged.

Why: enigo still requires the `windows` 0.61 helper family, while the rest of
the graph (wgpu, xcap, gpu-allocator, sysinfo >= 0.38) is on 0.62 — two copies
of `windows`, `windows-core`, `windows-future`, … in `Cargo.lock`, worth nine
`cargo deny` duplicate warnings. Upstream fixed exactly this in a one-line
dependabot bump of its own — `enigo-rs/enigo@a9da636`
("build(deps): update windows requirement from 0.61 to 0.62") — it just has
not been released (latest release is still 0.6.1, 2025-08-28).

How it is wired: the root `Cargo.toml` carries

```toml
[patch.crates-io]
enigo = { path = "vendor/enigo" }
```

and this directory is excluded from the workspace. The copy keeps version
0.6.1 on purpose: it replaces the registry 0.6.1 that satisfies
egui-elements' `enigo = "0.6"` requirement.

Removal: when a released enigo carries the bump (watch 0.6.x/0.7), delete
`vendor/`, the `[patch.crates-io]` entry and the `exclude` entry, then run
`cargo update -p enigo`.

Diff from the published package: `Cargo.toml` only — the one version line plus a
`[lints.rust] dead_code = "allow"` entry. Path dependencies are not lint-capped
the way registry builds are, so enigo's own unused `ModifierBitflag` alias on
non-Windows targets would otherwise surface in every consumer build; the
allowance keeps those builds as quiet as they were. `.cargo-ok`,
`.cargo_vcs_info.json`, `Cargo.toml.orig` and the package's own `Cargo.lock`
were dropped as packaging metadata.
