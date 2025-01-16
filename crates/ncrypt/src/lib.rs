pub mod credentials;
pub mod encrypt;
pub mod decrypt;
pub mod prelude;

pub use anyhow::anyhow;
pub use argon2::Argon2;
pub use zeroize;


pub fn extract_metadata(encrypted_data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
    let metadata_length = u32::from_le_bytes(
        encrypted_data[8..12].try_into().map_err(|e| anyhow!("Failed to parse metadata length {}", e))?
    );

    let metadata_start = 12;
    let metadata_end = metadata_start + (metadata_length as usize);
    Ok(encrypted_data[metadata_start..metadata_end].to_vec())
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct EncryptedInfo {
    pub password_salt: String,
    pub username_salt: String,
    pub cipher_nonce: Vec<u8>,
    pub argon2_params: Argon2Params,
}

impl EncryptedInfo {
    pub fn new(
        password_salt: String,
        username_salt: String,
        cipher_nonce: Vec<u8>,
        argon2_params: Argon2Params
    ) -> Self {
        Self {
            password_salt,
            username_salt,
            cipher_nonce,
            argon2_params,
        }
    }

    pub fn from_file(dir: &std::path::PathBuf) -> Result<Self, anyhow::Error> {
        let data = std::fs::read(dir)?;
        let metadata = extract_metadata(&data)?;

        let info: EncryptedInfo = bincode
            ::deserialize(&metadata)
            .map_err(|e| anyhow!("Deserialization failed {}", e))?;

        Ok(Self {
            password_salt: info.password_salt,
            username_salt: info.username_salt,
            cipher_nonce: info.cipher_nonce,
            argon2_params: info.argon2_params,
        })
    }
}

/// Argon2 parameters
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Argon2Params {
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
    pub hash_length: u64,
}

impl Argon2Params {
    pub fn new(m_cost: u32, t_cost: u32, p_cost: u32, hash_length: u64) -> Self {
        Self {
            m_cost,
            t_cost,
            p_cost,
            hash_length,
        }
    }

    pub fn from_argon2(argon2: Argon2) -> Result<Self, anyhow::Error> {
        let hash_lenght = argon2.params().output_len();

        if hash_lenght.is_none() {
            return Err(anyhow!("Failed to get output length"));
        }

        Ok(Self {
            m_cost: argon2.params().m_cost(),
            t_cost: argon2.params().t_cost(),
            p_cost: argon2.params().p_cost(),
            hash_length: hash_lenght.unwrap() as u64,
        })
    }
}

// Argon2Params Presets
impl Argon2Params {
    pub fn very_fast() -> Self {
        Self {
            m_cost: 64_000,
            t_cost: 3,
            p_cost: 8,
            hash_length: 64,
        }
    }

    pub fn fast() -> Self {
        Self {
            m_cost: 128_000,
            t_cost: 15,
            p_cost: 2,
            hash_length: 64,
        }
    }

    pub fn balanced() -> Self {
        Self {
            m_cost: 256_000,
            t_cost: 20,
            p_cost: 8,
            hash_length: 64,
        }
    }

    pub fn slow() -> Self {
        Self {
            m_cost: 512_000,
            t_cost: 20,
            p_cost: 8,
            hash_length: 64,
        }
    }

    pub fn very_slow() -> Self {
        Self {
            m_cost: 1024_000,
            t_cost: 30,
            p_cost: 8,
            hash_length: 64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::prelude::*;

    #[test]
    fn can_encrypt_decrypt() {
        let some_data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let credentials = Credentials::new(
            "username".to_string(),
            "password".to_string(),
            "password".to_string()
        );
        
        let argon_params = Argon2Params {
            m_cost: 24_000,
            t_cost: 3,
            p_cost: 1,
            hash_length: 64
        };

        let encrypted_data = encrypt_data(
            argon_params,
            some_data.clone(),
            credentials.clone()
        ).expect("Failed to encrypt data");

        std::fs
            ::write("test.ncrypt", &encrypted_data)
            .expect("Failed to write encrypted data to file");

        let encrypted_data = std::fs
            ::read("test.ncrypt")
            .expect("Failed to read encrypted data from file");

        let decrypted_data = decrypt_data(encrypted_data, credentials).expect(
            "Failed to decrypt data"
        );

        assert_eq!(some_data, decrypted_data);

        std::fs::remove_file("test.ncrypt").expect("Failed to remove test file");
    }
}
