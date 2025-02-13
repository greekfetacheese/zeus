use std::str::FromStr;
use ncrypt::zeroize::Zeroize;

use zeus_eth::alloy_primitives::hex::encode;
use alloy_signer::k256::ecdsa::SigningKey;
use alloy_signer_local::{ LocalSigner, PrivateKeySigner };

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

impl Drop for Wallet {
    fn drop(&mut self) {
        self.erase_key();
    }
}

impl Wallet {
    /// Erase the wallet's key from memory
    pub fn erase_key(&mut self) {
        unsafe {
            // Get a mutable pointer to the key
            let key_ptr: *mut LocalSigner<SigningKey> = &mut self.key;

            // Convert the key to a byte slice
            let key_bytes: &mut [u8] = std::slice::from_raw_parts_mut(
                key_ptr as *mut u8,
                std::mem::size_of::<LocalSigner<SigningKey>>()
            );

            // Zeroize the byte slice
            key_bytes.zeroize();
        }
    }

    /// Is the key erased?
    pub fn is_key_erased(&self) -> bool {
        let mut key_bytes = self.key.to_bytes();
        let erased = key_bytes.iter().all(|&b| b == 0);
        key_bytes.zeroize();
        erased
    }

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

impl Drop for WalletData {
    fn drop(&mut self) {
        self.key.zeroize();
    }
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

#[cfg(test)]
mod tests {
    use super::Wallet;

    #[test]
    fn test_key_erase() {
        let mut wallet = Wallet::new_rng("Test Wallet".to_string());
        let key_str = wallet.key_string();
        let original_key = wallet.key.clone();
        println!("Key: {}", key_str);
        assert!(!wallet.is_key_erased());

        wallet.erase_key();
        let erased_key_str = wallet.key_string();
        let erased_key = wallet.key.clone();
        println!("Erased Key: {}", erased_key_str);

        assert_ne!(original_key, erased_key);
        assert!(wallet.is_key_erased());
    }
}