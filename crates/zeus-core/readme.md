# Zeus-Core

The wallet domain that used to live in the binary's `src/core`: the `ZeusContext` and its
managers, the transaction and signing pipelines, the vault, wallet state and the `data/`
persistence catalog. It is a plain library with **no GUI code** — no `egui`, `eframe`, `rfd` or
other windowing dependency is compiled into it. User prompts (transaction confirmation, signing
requests, yes/no questions, notifications) reach the interface through the `UiHost` port in
`zeus_core::ui`: the binary installs its implementation once at startup, and a build with no host
attached refuses the prompt instead of waiting for an answer nobody can give.

## Part of [Zeus](https://github.com/greekfetacheese/zeus).
