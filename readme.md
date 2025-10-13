# <p align="center">Zeus</p>

## <p align="center"><b>Ethereum and EVM compatible desktop wallet.</b></p>

![Screenshot](src/zeus-home.png)
 
 ---

 ### Zeus is also available on [radicle](https://app.radicle.xyz/nodes/seed.radicle.garden/rad:zNS8uWXgE8r87Zb8ito1wzg84gbc)
 ### RID: `rad:zNS8uWXgE8r87Zb8ito1wzg84gbc`
 
 
## Installation
**You may need to install [Rust](https://www.rust-lang.org/tools/install)**

1. Download the latest release from the [Releases](https://github.com/greekfetacheese/zeus/releases)
2. Zeus is portable, you just need to extract the folder and run the `zeus` executable.

**Zeus saves and loads its data from the current directory it exists, so if you want to move it move it with the entire folder**


## Supported Chains
- Ethereum Mainnet
- Optimism
- Arbitrum
- Base
- Binance Smart Chain

## Supported Platforms
- Windows
- Linux

## Minimum System Requirements
- RAM: 8GB (For wallet recovery)

---

## How Wallet management work in Zeus
Zeus uses an hierarchical deterministic wallet which is derived from a username and a password, this operation is very time consuming
and on most machines it may take 10-15 minutes to complete.
After the creation of the HD wallet a `vault.data` file is created inside the `data` folder which contains the encrypted wallets and any saved contacts for faster access.
The `vault.data` file is encrypted with the credentials you provided when creating the wallet, but it can be changed later.
You can also import a wallet from a private key or a mnemonic phrase, but if you lose the `vault.data` file you will lose access to those wallets.

To create a new wallet it is better to simple derive a new child wallet from the master wallet, this way you can have as much wallets
you want under the same master wallet which can be recovered from the same username and password even if you lose the `vault.data` file.

---


## Features

- **Connect to dapps:** Still WIP, some dapps work well, some don't.
- **Wallet Management:** Import, export and create new wallets under the same master wallet.
- **Crypto Transactions:** Send ETH and ERC-20 tokens.
- **Cross-Chain Bridging:** Bridge ETH between the supported chains using [Across](https://across.to/) (**BNB is not supported**).
- **Basic Portfolio Tracking:** Monitor your assets with a simple interface.
- **Swap Tokens:** Swap tokens on the Uniswap protocol (through the [Universal Router](https://docs.uniswap.org/contracts/v4/deployments)).
- **Transaction Simulations:** Zeus run local EVM simulations using [revm](https://github.com/bluealloy/revm) to verify transactions before you submit them, what you see on the screen is what you will get.
- **MEV Protect:** For transactions that are vulnerable to MEV by default Zeus uses mev-protect rpc endpoints (ETH mainnet only).

 Zeus has been designed to work with what the Ethereum RPC API provides, it does not rely on any kind of 3rd-party service to work, you simply give it an rpc endpoint and its ready to go.

 Because of that it does not automatically index data like token approvals, balances etc..

 By default it uses free public rpc endpoints obtained from [Chainlist.org](https://chainlist.org/).
 You can of course bring your own endpoints and disable the default ones

---

## Security
- **Strong encryption:** Zeus uses the [Argon2Id](https://github.com/P-H-C/phc-winner-argon2) as a KDF and [xChaCha20Poly1305](https://en.wikipedia.org/wiki/ChaCha20-Poly1305) as the encryption algorithm to derive the master wallet and encrypt the `Vault`.
- **No private key leaks:** Zeus uses [secure-types](https://github.com/greekfetacheese/secure-types) to handle private keys and other sensintive data in-memory. This makes it extremely hard for an attacker/malware to extract any private keys. (Unlike browser extension wallets)
- **No telemetry:** Zeus does not need neither communicates with any third-party server, everything you do stays local on your computer.

## Bugs/Issues
I'm not aware of any major bugs, feel free to open an issue if you find any.

---


## Credits
Zeus wouldn't be possible without:
- [alloy-rs](https://github.com/alloy-rs/alloy)
- [revm](https://github.com/bluealloy/revm)
- [egui](https://github.com/emilk/egui)


## Build from source

For development:
```
cargo build --features dev
```

For release:
```
cargo build --profile prod
```