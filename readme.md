## Zeus - Ethereum Desktop Wallet

![Screenshot](src/zeus-home.png)

### Zeus is an Ethereum Desktop Wallet with a focus on simplicity and security.

## Supported Chains
- Ethereum
- Optimism
- Binance Smart Chain
- Base Chain
- Arbitrum

## Supported Platforms
- Windows
- MacOS
- Linux


## Features

### Zeus is still in heavy development, so as for now the features are limited to:
- **Wallet Management:** Import and manage your wallets.
- **Crypto Transactions:** Send ETH and ERC-20 tokens.
- **Basic Portfolio Tracking:** Monitor your assets with a simple interface.


### Currently Zeus does not use an indexer, it does not rely on any **Third-Party API** to index your wallet balances etc...
### That also means you need to manually add any tokens to each wallet if you want to track them
### By default it uses free public rpc endpoints obtained from [Chainlist.org](https://chainlist.org/)
### You can of course bring your own endpoints and disable the default ones

### I'm not aware of any major bugs, but this is still work in progress and not audited so use at your own risk.

## Privacy

### There is zero telemetry in the app, everything you do stays locally on your computer.
### If you want maximum privacy you should consider using your own rpc endpoint or use a **good** VPN like [Mullvad](https://mullvad.net/)

### Things that i want to do in the future:
- **The ability to backup the account wallets in popular clouds like Google Drive, Dropbox, etc...**
- **Full integration of the Uniswap protocol**
- **Bridge between chains**
- **Connect to dApps**


## Build from source

For development:
```
cargo build --profile dev --features dev
```

For release:
```
cargo build --profile maxperf
```