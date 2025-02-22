use std::str::FromStr;
use ncrypt_me::zeroize::Zeroize;

use zeus_eth::alloy_primitives::hex::encode;
use zeus_eth::wallet::SafeSigner;
use alloy_signer_local::PrivateKeySigner;

/// User Wallet
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Wallet {
    /// Name of the wallet (if empty, we generate a name)
    pub name: String,

    pub notes: String,

    /// Hide this wallet from the GUI?
    pub hidden: bool,

    /// The key of the wallet
    pub key: SafeSigner,
}

impl Wallet {

    pub fn is_key_erased(&self) -> bool {
        self.key.is_erased()
    }

    /// Return the wallet's key in string format
    pub fn key_string(&self) -> String {
        let key_vec = self.key.inner().to_bytes().to_vec();
        encode(key_vec)
    }

    /// Create a new wallet from a random private key
    pub fn new_rng(name: String) -> Self {
        let key = SafeSigner::random();

        Self {
            name,
            notes: String::new(),
            hidden: false,
            key,
        }
    }

    /// Create a new wallet from a given private key
    pub fn new_from_key(name: String, notes: String, hidden: bool, mut key_str: String) -> Result<Self, anyhow::Error> {
        let key = PrivateKeySigner::from_str(&key_str)?;
        let key = SafeSigner::from(key);
        key_str.zeroize();

        Ok(Self {
            name,
            notes,
            hidden,
            key,
        })
    }

    pub fn address(&self) -> String {
        self.key.inner().address().to_string()
    }

    /// Get the wallet address truncated
    pub fn address_truncated(&self) -> String {
        let address = self.key.inner().address().to_string();
        format!("{}...{}", &address[..6], &address[36..])
    }
}