use std::str::FromStr;
use ncrypt::zeroize::Zeroize;

use zeus_eth::alloy_primitives::hex::encode;
use zeus_eth::alloy_signer::k256::ecdsa::SigningKey;
use zeus_eth::alloy_signer_local::{LocalSigner, PrivateKeySigner};



/// User Wallet
#[derive(Clone, PartialEq)]
pub struct Wallet {
    /// Name of the wallet (if empty, we generate a name)
    pub name: String,

    pub notes: String,

    /// Hide this wallet from the GUI?
    pub hidden: bool,

    /// The key of the wallet
    pub key: LocalSigner<SigningKey>,
}

impl Wallet {  

    /// Return the wallet's key in string format
    pub fn key_string(&self) -> String {
        let key_vec = self.key.to_bytes().to_vec();
        encode(key_vec)
    }

    /// Create a new wallet from a random private key
    pub fn new_rng(name: String) -> Self {
        let key = PrivateKeySigner::random();

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
        key_str.zeroize();

        Ok(Self {
            name,
            notes,
            hidden,
            key,
        })
    }

    pub fn address(&self) -> String {
        self.key.address().to_string()
    }


    /// Get the wallet address truncated
    pub fn address_truncated(&self) -> String {
        let address = self.key.address().to_string();
        format!("{}...{}", &address[..6], &address[36..])
    }

}


/// Helper struct to serialize [Wallet] in JSON
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct WalletData {
    pub name: String,
    pub notes: String,
    pub hidden: bool,
    pub key: String,
}

impl WalletData {
    pub fn new(name: String, notes: String, hidden: bool, key: String) -> Self {
        Self {
            name,
            notes,
            hidden,
            key,
        }
    }

    /// Convert to [Wallet]
    pub fn to_wallet(&self) -> Result<Wallet, anyhow::Error> {
        let wallet = Wallet::new_from_key(self.name.clone(), self.notes.clone(), self.hidden, self.key.clone())?;
        Ok(wallet)
    }

    /// Serialize to JSON String
    pub fn serialize(&self) -> Result<String, anyhow::Error> {
        Ok(serde_json::to_string(self)?)
    }

    /// Deserialize from slice
    pub fn from_slice(data: &[u8]) -> Result<Self, anyhow::Error> {
        Ok(serde_json::from_slice::<WalletData>(data)?)
    }
}