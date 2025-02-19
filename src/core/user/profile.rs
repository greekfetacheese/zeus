use ncrypt_me::{ zeroize::Zeroize, decrypt_data, encrypt_data, Argon2Params, Credentials };
use zeus_eth::alloy_primitives::Address;
use crate::core::utils::data_dir;
use super::wallet::{ Wallet, WalletData };
use anyhow::anyhow;
use std::path::PathBuf;

pub const PROFILE_FILE: &str = "profile.data";



/// Helper struct to serialize [Profile] in JSON
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ProfileData {
    pub wallets: Vec<WalletData>,
}

impl Drop for ProfileData {
    fn drop(&mut self) {
        self.erase();
    }
}

impl ProfileData {
    pub fn new(wallets: Vec<WalletData>) -> Self {
        Self {
            wallets,
        }
    }

    /// Erase the wallets from memory by zeroing out the keys
    pub fn erase(&mut self) {
        for wallet in self.wallets.iter_mut() {
            wallet.key.zeroize();
        }
    }

    /// Serialize to JSON String
    pub fn serialize(&self) -> Result<String, anyhow::Error> {
        Ok(serde_json::to_string(self)?)
    }

    /// Deserialize from slice
    pub fn from_slice(data: &[u8]) -> Result<Self, anyhow::Error> {
        Ok(serde_json::from_slice::<ProfileData>(data)?)
    }
}

/// Main user profile struct
#[derive(Clone)]
pub struct Profile {
    pub credentials: Credentials,

    /// The wallets of the profile
    pub wallets: Vec<Wallet>,

    /// The current selected wallet from the GUI
    pub current_wallet: Wallet,
}

impl Default for Profile {
    fn default() -> Self {
        let wallet = Wallet::new_rng("Wallet 1".to_string());
        let wallets = vec![wallet.clone()];
        Self {
            credentials: Credentials::default(),
            wallets,
            current_wallet: wallet,
        }
    }
}

impl Profile {
    pub fn new_wallet_from_key(&mut self, mut name: String, key: String) -> Result<(), anyhow::Error> {
        if !name.is_empty() {
            if self.wallet_name_exists(&name) {
                return Err(anyhow!("Wallet with name {} already exists", name));
            }
        }

        if name.is_empty() {
            name = self.generate_wallet_name();
        }

        let wallet = Wallet::new_from_key(name, String::new(), false, key)?;
        self.wallets.push(wallet);
        Ok(())
    }

    pub fn new_wallet_rng(&mut self, mut name: String) -> Result<(), anyhow::Error> {
        if !name.is_empty() {
            if self.wallet_name_exists(&name) {
                return Err(anyhow!("Wallet with name {} already exists", name));
            }
        } else {
            name = self.generate_wallet_name();
        }

        let wallet = Wallet::new_rng(name);
        self.wallets.push(wallet);
        Ok(())
    }

    pub fn generate_wallet_name(&self) -> String {
        let mut starter_number = 1;
        loop {
            let dummy_name = format!("Wallet {}", starter_number);
            if !self.wallet_name_exists(&dummy_name) {
                return dummy_name;
            }
            starter_number += 1;
        }
    }

    /// Check if the given wallet name already exists
    pub fn wallet_name_exists(&self, name: &str) -> bool {
        self.wallets.iter().any(|w| w.name == name)
    }
    

    /// Encrypt the profile and save it to a file
    pub fn encrypt_and_save(&self, dir: &PathBuf, argon: Argon2Params) -> Result<(), anyhow::Error> {
        // ! make sure we dont accidentally erased any of the wallet keys
        for wallet in self.wallets.iter() {
            if wallet.is_key_erased() {
                return Err(anyhow!("Wallet key is erased"));
            }
        }
        let profile_data = self.serialize()?.as_bytes().to_vec();
        let encrypted_data = encrypt_data(argon, profile_data, self.credentials.clone())?;
        std::fs::write(dir, encrypted_data)?;
        Ok(())
    }

    pub fn decrypt(&self, dir: &PathBuf) -> Result<Vec<u8>, anyhow::Error> {
        let encrypted_data = std::fs::read(dir)?;
        let decrypted_data = decrypt_data(encrypted_data, self.credentials.clone())?;
        Ok(decrypted_data)
    }

    /// Same as [Self::decrypt()] but make sure we zero out the decrypted data
    /// 
    /// Useful for verifying credentials
    pub fn decrypt_zero(&self, dir: &PathBuf) -> Result<(), anyhow::Error> {
        let mut decrypted_data = self.decrypt(dir)?;
        decrypted_data.zeroize();
        Ok(())
    }

    /// Decrypt and load the profile
    pub fn decrypt_and_load(&mut self, dir: &PathBuf) -> Result<(), anyhow::Error> {
        let decrypted_data = self.decrypt(dir)?;
        let profile_data = ProfileData::from_slice(&decrypted_data)?;

        let mut wallets = Vec::new();
        for wallet in &profile_data.wallets {
            wallets.push(wallet.to_wallet()?);
        }

        self.wallets = wallets;

        // if there is at least 1 wallet available, set the current wallet to the first one
        if !self.wallets.is_empty() {
            self.current_wallet = self.wallets[0].clone();
        }

        Ok(())
    }

    /// Store all data to [ProfileData] and serialize it to a JSON string
    pub fn serialize(&self) -> Result<String, anyhow::Error> {
        let mut wallet_data = Vec::new();
        for wallet in self.wallets.iter() {
            let key = wallet.key_string();
            let data = WalletData::new(wallet.name.clone(), wallet.notes.clone(), wallet.hidden, key);
            wallet_data.push(data);
        }

        let profile_data = ProfileData::new(wallet_data);
        let serialized = profile_data.serialize()?;

        Ok(serialized)
    }

    pub fn remove_wallet(&mut self, wallet: Wallet) {
        self.wallets.retain(|w| w != &wallet);
    }

    /// Get the current wallet address
    pub fn wallet_address(&self) -> Address {
        self.current_wallet.key.address()
    }

    /// Is a profile exists at the data directory
    pub fn exists() -> Result<bool, anyhow::Error> {
        let dir = data_dir()?.join(PROFILE_FILE);
        Ok(dir.exists())
    }

    /// Profile directory
    pub fn profile_dir() -> Result<PathBuf, anyhow::Error> {
        Ok(data_dir()?.join(PROFILE_FILE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_profile() {
        let wallet_1 = Wallet::new_rng("Wallet 1".to_string());
        let wallet_2 = Wallet::new_rng("Wallet 2".to_string());

        let original_key1 = wallet_1.key_string();
        let original_key2 = wallet_2.key_string();

        let credentials = Credentials::new("test".to_string(), "password".to_string(), "password".to_string());

        let mut profile = Profile {
            credentials,
            wallets: vec![wallet_1.clone(), wallet_2],
            current_wallet: wallet_1,
        };

        let argon_2 = Argon2Params::very_fast();
        let dir = PathBuf::from("profile_test.data");
        profile.encrypt_and_save(&dir, argon_2).expect("Profile Encryption failed");

        profile.decrypt_and_load(&dir).expect("Profile Recovery failed");

        let recovered_wallet_1 = profile.wallets.get(0).unwrap();
        let recovered_wallet_2 = profile.wallets.get(1).unwrap();

        assert_eq!(recovered_wallet_1.name, "Wallet 1");
        assert_eq!(recovered_wallet_2.name, "Wallet 2");

        let key_1 = recovered_wallet_1.key_string();
        let key_2 = recovered_wallet_2.key_string();

        assert_eq!(key_1, original_key1);
        assert_eq!(key_2, original_key2);

        fs::remove_file("profile_test.data").unwrap();
    }
}
