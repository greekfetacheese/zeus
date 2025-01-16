use argon2::{
    password_hash::{PasswordHasher, SaltString},
    Algorithm, Argon2, Params, Version,
};
use chacha20poly1305::aead::{generic_array::GenericArray, Aead, Payload};

use super::{
    credentials::Credentials,
    encrypt::{xchacha20_poly_1305, HEADER},
    EncryptedInfo,
};

use anyhow::anyhow;

/// Decrypts the data using the provided credentials
///
/// ### Arguments
///
/// - `data` - The data to decrypt
/// - `credentials` - The credentials to use for decryption
pub fn decrypt_data(data: Vec<u8>, credentials: Credentials) -> Result<Vec<u8>, anyhow::Error> {

        // Verify Header
        if &data[0..8] != HEADER {
            return Err(anyhow!("Header not found, invalid file format?"));
        }
    
        // Read Metadata Length
        let metadata_length = u32::from_le_bytes(
            data[8..12].try_into().map_err(|e| anyhow!("Failed to parse metadata length {}", e))?,
        );
    
        // Extract Metadata
        let metadata_start = 12;
        let metadata_end = metadata_start + metadata_length as usize;
        let metadata_bytes = &data[metadata_start..metadata_end];
    
        let info: EncryptedInfo = bincode::deserialize(metadata_bytes).map_err(|e| anyhow!("Deserialization failed {}", e))?;
    
        // Extract Encrypted Data
        let encrypted_data = &data[metadata_end..];

    let decrypted_data = decrypt(credentials, info, encrypted_data.to_vec())?;
    Ok(decrypted_data)
}

fn decrypt(mut credentials: Credentials, info: EncryptedInfo, data: Vec<u8>) -> Result<Vec<u8>, anyhow::Error> {
    credentials.is_valid()?;

    let params = Params::new(
        info.argon2_params.m_cost,
        info.argon2_params.t_cost,
        info.argon2_params.p_cost,
        Some(info.argon2_params.hash_length as usize),
    )
    .map_err(|e| anyhow!("Failed to create Argon2 params {:?}", e))?;

    let argon2 = Argon2::new(Algorithm::default(), Version::default(), params.clone());

    let password_salt = SaltString::from_b64(&info.password_salt)
        .map_err(|e| anyhow!("Failed to parse password salt {:?}", e))?;

    let password_hash = argon2
        .hash_password(credentials.password().as_bytes(), &password_salt)
        .map_err(|e| anyhow!("Failed to hash password {:?}", e))?;

    let key = password_hash
        .hash
        .ok_or(anyhow!("Failed to get the password hash output"))?;

    // create the cipher using the hashed password as the key
    let cipher = xchacha20_poly_1305(key);

    let username_salt = SaltString::from_b64(&info.username_salt)
        .map_err(|e| anyhow!("Failed to parse username salt {:?}", e))?;

    let username_hash = argon2
        .hash_password(credentials.username().as_bytes(), &username_salt)
        .map_err(|e| anyhow!("Failed to hash username {:?}", e))?;

    credentials.erase();

    let aad = username_hash
        .hash
        .ok_or(anyhow!("Failed to get the username hash output"))?;

    let payload = Payload {
        msg: data.as_ref(),
        aad: aad.as_bytes(),
    };

    
    let nonce = GenericArray::from_slice(&info.cipher_nonce);

    let decrypted_data = cipher
        .decrypt(nonce, payload)
        .map_err(|e| anyhow!("Failed to decrypt data {:?}", e))?;

    Ok(decrypted_data)
}