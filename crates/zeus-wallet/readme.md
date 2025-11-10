# <p align="center">Zeus-Wallet</p>

## Implementation of an Hierarchical Deterministic Wallet (BIP32) that can be derived from a username and password.

## Part of [Zeus](https://github.com/greekfetacheese/zeus).


# Usage

```rust
use zeus_wallet::{SecureHDWallet, derive_seed};
use secure_types::SecureString;

let username = SecureString::from("username");
let password = SecureString::from("password");

// This is just an example, in reality you should use way higher values
let m_cost = 64_000; // 64 MB of memory
let t_cost = 8; // 8 iterations
let p_cost = 1; // 1 parallel threads

// Derive the seed from the given username and password that will be used to derive the HD wallet
// based on the BIP32 standard
let seed = derive_seed(&username, &password, m_cost, t_cost, p_cost).unwrap();

let hd_wallet = SecureHDWallet::new_from_seed(Some("My Wallet".to_string()), seed);

println!("Wallet Address: {}", hd_wallet.master_wallet.address());

      // Generate 10 child wallets using the master wallet
      for i in 0..10 {
         let name = format!("Child Wallet {}", i);
         hd_wallet.derive_child(name).unwrap();
      }

        for (i, children) in hd_wallet.children.iter().enumerate() {
         assert!(!children.is_master());
         assert!(!children.is_imported());
         assert!(children.is_hardened());
         assert!(children.is_child());

         let path = children.derivation_path_string();
         eprintln!(
            "Child: {} Path: {} Address: {}",
            i,
            path,
            children.address()
         );
      }

```