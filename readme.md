# Zeus - Ethereum Desktop Wallet

![Screenshot](src/zeus-home.png)

 Zeus is an Ethereum Desktop Wallet with a focus on simplicity and security.
 
 ---

## Supported Chains
- Ethereum
- Optimism
- Binance Smart Chain
- Base Chain
- Arbitrum


## Supported Platforms
- Windows
- Linux
- MacOS (Soon)

---

## Features

### Zeus is still in heavy development, so as for now the features are limited to:
- **Wallet Management:** Import and manage your wallets.
- **Crypto Transactions:** Send ETH and ERC-20 tokens.
- **Basic Portfolio Tracking:** Monitor your assets with a simple interface.


 Currently Zeus does not use an indexer, it does not rely on any **Third-Party API** to index your wallet balances etc...
 
 That also means you need to manually add any tokens to each wallet if you want to track them
 By default it uses free public rpc endpoints obtained from [Chainlist.org](https://chainlist.org/).
 
 You can of course bring your own endpoints and disable the default ones

---

## Bugs & Security
> **Disclaimer** I'm not aware of any major bugs, but this is still work in progress and **not audited** so use at your own risk.

---

## Privacy
- **Zero telemetry**: Everything you do stays local on your computer.
- For maximum privacy:
  - Use your own RPC endpoint.
  - Use a **privacy-focused** VPN like [Mullvad](https://mullvad.net/).
 
 ---

## Roadmap
Features I plan to implement:
- Backup account wallets to popular cloud services (e.g., Google Drive, Dropbox).
- Full integration with the Uniswap protocol.
- Cross-chain bridging.
- Connect to dApps.

---

## Notes
The GUI for Zeus is made with [egui](https://github.com/emilk/egui).

This has some advantages like really fast and smooth rendering of the UI, no need to write a lot of UI or callback code to handle app logic compared to other frameworks.

Right now the biggest drawback from using a library like **egui** its the difficulty to propely align/layout a UI due to its nature, some parts of the UI may glitch or not aligned properly this is because making a nice modern UI in **egui** is harder than i thought.

---



## Build from source

For development:
```
cargo build --profile dev --features dev
```

For release:
```
cargo build --profile maxperf
```