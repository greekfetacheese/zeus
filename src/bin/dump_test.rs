use ncrypt_me::{ zeroize::Zeroize, Credentials};
use std::io::Write;
use secure_types::SecureString;

use chacha20poly1305::{
   AeadCore, KeyInit, XChaCha20Poly1305, XNonce,
   aead::{Aead, rand_core::RngCore, OsRng, Payload}
};

//#[path = "../core/mod.rs"]
//mod core;

pub struct EncryptedCredentials {
   pub encrypted_username: Vec<u8>,
   pub encrypted_password: Vec<u8>,
   pub key: CipherKey
}

impl EncryptedCredentials {
   pub fn new(credentials: Credentials, key: CipherKey) -> Self {
      let payload = Payload {
         msg: credentials.username().as_bytes(),
         aad: &key.aad
      };

      let encrypted_username = key.cipher.encrypt(&key.nonce, payload).unwrap();

      let payload = Payload {
         msg: credentials.password().as_bytes(),
         aad: &key.aad
      };

      let encrypted_password = key.cipher.encrypt(&key.nonce, payload).unwrap();

      /* 
      let mut bytes = [0u8; 128];
      let mut bytes2 = [0u8; 128];
      OsRng.fill_bytes(&mut bytes);
      OsRng.fill_bytes(&mut bytes2);
      let payload = Payload {
         msg: &bytes,
         aad: &bytes2
      };
      */

      Self {
         encrypted_username,
         encrypted_password,
         key
      }
   }

   /// Decrypt the encrypted credentials
   pub fn decrypt(&self) -> Result<Credentials, anyhow::Error> {
      let payload = Payload {
         msg: &self.encrypted_username,
         aad: &self.key.aad
      };

      let username = self.key.cipher.decrypt(&self.key.nonce, payload).unwrap();

      let payload = Payload {
         msg: &self.encrypted_password,
         aad: &self.key.aad
      };

      let password = self.key.cipher.decrypt(&self.key.nonce, payload).unwrap();

      let credentials = Credentials::new(
         String::from_utf8(username)?,
         String::from_utf8(password.clone())?,
         String::from_utf8(password)?
      );

      Ok(credentials)
   }
}

#[derive(Clone, Debug)]
pub struct SecureCredentials {
   pub username: SecureString,
   pub password: SecureString,
}

impl SecureCredentials {
   pub fn new(username: SecureString, password: SecureString) -> Self {
      Self {
         username,
         password
      }
   }
}


#[derive(Clone)]
pub struct CipherKey {
   pub plain_text: String,
   pub cipher: XChaCha20Poly1305,
   pub nonce: XNonce,
   pub aad: [u8; 128]
}

impl CipherKey {
   pub fn rng() -> Self {
      let key = XChaCha20Poly1305::generate_key(&mut OsRng);
      let cipher = XChaCha20Poly1305::new(&key);
      let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
      let mut aad = vec![0u8; 128];
      OsRng.fill_bytes(&mut aad);
      let aad = aad.try_into().unwrap();
      println!("aad: {:?}", aad);
      
      Self {
         plain_text: "search for me".to_string(),
         cipher,
         nonce,
         aad
      }
   }
}

fn main() -> Result<(), anyhow::Error> {


   let mut credentials = Credentials::default();
   prompt("Username: ", credentials.user_mut())?;
   prompt("Password: ", credentials.passwd_mut())?;
   prompt("Confirm Password: ", credentials.confirm_passwd_mut())?;

   let key = CipherKey::rng();
   
   let encrypted_credentials = EncryptedCredentials::new(credentials, key.clone());
   let time = std::time::Instant::now();
   let credentials = encrypted_credentials.decrypt()?;
   println!("Decryption took {} ms", time.elapsed().as_millis());
   
   let mut username = SecureString::from("");
   prompt("Username: ", username.borrow_mut())?;
   let mut password = SecureString::from("");
   prompt("Password: ", password.borrow_mut())?;

   let secure_credentials = SecureCredentials::new(username, password);
   
   


   loop {
      std::thread::sleep(std::time::Duration::from_millis(500));
   }
}

fn prompt(msg: &str, string: &mut str) -> Result<(), anyhow::Error> {
   print!("{}", msg);
   std::io::stdout().flush().unwrap();

   std::io::stdin().read_line(string)?;
   Ok(())
}
