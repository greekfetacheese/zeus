use crate::core::user::Account;
use crate::gui::SHARED_GUI;
use ncrypt_me::EncryptedInfo;
use std::path::PathBuf;
use secure_types::SecureString;

// Shortcuts for functions that may fail but we need to show an error message
// These functions are only called on a seperate thread so we dont block or panic the main thread


pub fn new_wallet_from_key_or_phrase(account: &mut Account, name: String, from_key: bool, key: SecureString) {
   match account.new_wallet_from_key_or_phrase(name, from_key, key) {
      Ok(_) => {}
      Err(e) => {
         {
            let mut gui = SHARED_GUI.write().unwrap();
            gui.open_msg_window("Failed to import wallet", e.to_string());
         }
         panic!("Failed to import wallet");
      }
   }
}

pub fn new_wallet_rng(account: &mut Account, name: String) {
   match account.new_wallet_rng(name) {
      Ok(_) => {}
      Err(e) => {
         {
            let mut gui = SHARED_GUI.write().unwrap();
            gui.open_msg_window("Failed to create new wallet", e.to_string());
         }
         panic!("Failed to create new wallet");
      }
   }
}

pub fn get_account_dir() -> PathBuf {
   match Account::dir() {
      Ok(dir) => dir,
      Err(e) => {
         {
            let mut gui = SHARED_GUI.write().unwrap();
            gui.open_msg_window("Failed to get profile directory", e.to_string());
         }
         panic!("Failed to get profile directory");
      }
   }
}

pub fn get_encrypted_info(dir: &PathBuf) -> EncryptedInfo {
   match EncryptedInfo::from_file(&dir) {
      Ok(info) => info,
      Err(e) => {
         {
            let mut gui = SHARED_GUI.write().unwrap();
            gui.open_msg_window("Failed to get encrypted info", e.to_string());
         }
         panic!("Failed to get encrypted info");
      }
   }
}

pub fn verify_credentials(account: &mut Account) -> bool {
   let dir = get_account_dir();
   open_loading("Verifying credentials...".to_string());

   match account.decrypt_zero(&dir) {
      Ok(_) => {
         let mut gui = SHARED_GUI.write().unwrap();
         gui.loading_window.open = false;
         return true;
      }
      Err(e) => {
         let mut gui = SHARED_GUI.write().unwrap();
         gui.loading_window.open = false;
         gui.open_msg_window("Invalid Credentials", e.to_string());
         return false;
      }
   }
}

pub fn open_loading(msg: String) {
   let mut gui = SHARED_GUI.write().unwrap();
   gui.loading_window.open(msg);
}

pub fn close_loading() {
   let mut gui = SHARED_GUI.write().unwrap();
   gui.loading_window.reset();
}
