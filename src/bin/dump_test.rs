use ncrypt_me::{ zeroize::Zeroize, Credentials};
use std::io::Write;

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

      Self {
         encrypted_username,
         encrypted_password,
         key
      }
   }
}

pub struct Credentials2 {
   pub username: [u8; 1024],
   pub password: [u8; 1024],
   pub confirm_password: [u8; 1024],
}

impl Credentials2 {
   pub fn new(mut username: String, mut password: String, mut confirm_password: String) -> Self {
      let mut user = [0u8; 1024];
      let mut passwd = [0u8; 1024];
      let mut confirm = [0u8; 1024];

      user[..username.len()].copy_from_slice(username.as_bytes());
      passwd[..password.len()].copy_from_slice(password.as_bytes());
      confirm[..confirm_password.len()].copy_from_slice(confirm_password.as_bytes());

      username.zeroize();
      password.zeroize();
      confirm_password.zeroize();

      Self {
         username: user,
         password: passwd,
         confirm_password: confirm,
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

      
      Self {
         plain_text: "search for me".to_string(),
         cipher,
         nonce,
         aad: aad.try_into().unwrap()
      }
   }
}

fn main() -> Result<(), anyhow::Error> {
   let mut credentials = Credentials::default();
   prompt("Username: ", credentials.user_mut())?;
   prompt("Password: ", credentials.passwd_mut())?;
   credentials.copy_passwd_to_confirm();

   let key = CipherKey::rng();
   println!("Cipher key initialized");
   let _encrypted_credentials = EncryptedCredentials::new(credentials, key.clone());

   let mut username = String::new();
   prompt("A Different Username: ", &mut username)?;
   let mut password = String::new();
   prompt("A Different Password: ", &mut password)?;
   let mut confirm_password = String::new();
   prompt("Confirm Password: ", &mut confirm_password)?;
   let _credentials2 = Credentials2::new(username, password, confirm_password);


   println!("Done");

   loop {
      std::thread::sleep(std::time::Duration::from_millis(500));
   }
}

fn prompt(msg: &str, string: &mut String) -> Result<(), anyhow::Error> {
   print!("{}", msg);
   std::io::stdout().flush().unwrap();

   std::io::stdin().read_line(string)?;
   Ok(())
}
