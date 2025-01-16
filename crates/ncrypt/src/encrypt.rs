use argon2::{
    password_hash::{Output, PasswordHasher, SaltString},
    Algorithm, Argon2, Params, Version,
};
use chacha20poly1305::{
    aead::{generic_array::GenericArray, Aead, OsRng, Payload},
    AeadCore, KeyInit, XChaCha20Poly1305,
};

use super::credentials::Credentials;
use super::{Argon2Params, EncryptedInfo};
use anyhow::anyhow;
use bincode;


/*
█████████████████████████████████████████████████████████████████████████
█                                                                       █
█                           nCrypt File Format                         █
█                                                                       █
█    ┌───────────┬───────────────┬───────────────┬────────────────────┐ █
█    │ Header    │ Metadata Len  │ Metadata      │ Encrypted Data     │ █
█    │ 8 bytes   │ 4 bytes (LE)  │ Variable Size │ Variable Size      │ █
█    └───────────┴───────────────┴───────────────┴────────────────────┘ █
█                                                                       █
█   Details:                                                            █
█   - **Header**: A fixed 8-byte ASCII string identifying the format    █
█     and version (e.g., "nCrypt1\0").                                  █
█   - **Metadata Length**: A 4-byte unsigned integer in little-endian   █
█     format specifying the size of the metadata section.               █
█   - **Metadata**: Serialized metadata containing Argon2 parameters,  █
█     salt, nonce, etc. (encoded using `bincode`).                      █
█   - **Encrypted Data**: The raw encrypted data.                       █
█                                                                       █
█   Example (hex representation):                                       █
█   [6E 43 72 79 70 74 31 00]  [12 00 00 00]  [Serialized Metadata]    █
█   [Encrypted Data]                                                    █
█                                                                       █
█████████████████████████████████████████████████████████████████████████
*/


/// File Header
pub const HEADER: &[u8; 8] = b"nCrypt1\0";



/// Encrypts the given data using the provided credentials
///
/// ### Arguments
///
/// - `argon_params` - The Argon2 parameters to use for the password hashing
/// - `data` - The data to encrypt
/// - `credentials` - The credentials to use for encryption
pub fn encrypt_data(
    argon_params: Argon2Params,
    data: Vec<u8>,
    credentials: Credentials,
) -> Result<Vec<u8>, anyhow::Error> {
    let (encrypted_data, info) = encrypt(argon_params.clone(), credentials, data)?;

    let serialized_info = bincode::serialize(&info)?;

    // Construct the file format
    let mut result = Vec::new();

    // Append the header
    result.extend_from_slice(HEADER);

    // Append the metadata Length
    let metadata_length = serialized_info.len() as u32;
    result.extend_from_slice(&metadata_length.to_le_bytes());

    // Append the metadata
    result.extend_from_slice(&serialized_info);

    // Append the encrypted Data
    result.extend_from_slice(&encrypted_data);

    Ok(result)
}

/// Encrypts the given data using the provided credentials
fn encrypt(
    argon_params: Argon2Params,
    mut credentials: Credentials,
    data: Vec<u8>,
) -> Result<(Vec<u8>, EncryptedInfo), anyhow::Error> {
    credentials.is_valid()?;

    let params = Params::new(
        argon_params.m_cost,
        argon_params.t_cost,
        argon_params.p_cost,
        Some(argon_params.hash_length as usize),
    )
    .map_err(|e| anyhow!("Failed to set Argon2 params {:?}", e))?;

    let argon2 = Argon2::new(Algorithm::default(), Version::default(), params);

    let password_salt = SaltString::generate(&mut OsRng);
    let username_salt = SaltString::generate(&mut OsRng);


    // hash the password
    let password_hash = match argon2.hash_password(credentials.password().as_bytes(), &password_salt) {
        Ok(hash) => hash,
        Err(e) => {
            return Err(anyhow!("Failed to hash password {:?}", e));
        }
    };

    // get the hash output
    let key = password_hash
        .hash
        .ok_or(anyhow!("Failed to get the password hash output"))?;

    // hash the username for the AAD
    let username_hash = match argon2.hash_password(credentials.username().as_bytes(), &username_salt) {
        Ok(hash) => hash,
        Err(e) => return Err(anyhow!("Failed to hash username {:?}", e)),
    };

    credentials.erase();

    let aad = username_hash.hash.ok_or(anyhow!("Failed to get the username hash output"))?;


    // create the cipher using the hashed password as the key
    let cipher = xchacha20_poly_1305(key);
    let cipher_nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);


    let payload = Payload {
        msg: data.as_ref(),
        aad: aad.as_bytes(),
    };


    let encrypted_data = cipher
        .encrypt(&cipher_nonce, payload)
        .map_err(|e| anyhow!("Failed to encrypt data {:?}", e))?;

    let info = EncryptedInfo::new(password_salt.to_string(), username_salt.to_string(), cipher_nonce.to_vec(), argon_params);

    Ok((encrypted_data, info))
}

pub fn xchacha20_poly_1305(key: Output) -> XChaCha20Poly1305 {
    let key = GenericArray::from_slice(&key.as_bytes()[..32]);
    XChaCha20Poly1305::new(key)
}