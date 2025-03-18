use crate::core::user::Profile;
use crate::gui::SHARED_GUI;
use ncrypt_me::EncryptedInfo;
use std::path::PathBuf;
use secure_types::SecureString;

// Shortcuts for functions that may fail but we need to show an error message
// These functions are only called on a seperate thread so we dont block or panic the main thread


pub fn new_wallet_from_key(profile: &mut Profile, name: String, key: SecureString) {
   match profile.new_wallet_from_key(name, key) {
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

pub fn new_wallet_rng(profile: &mut Profile, name: String) {
   match profile.new_wallet_rng(name) {
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

pub fn get_profile_dir() -> PathBuf {
   match Profile::profile_dir() {
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

pub fn verify_credentials(profile: &mut Profile) -> bool {
   let dir = get_profile_dir();
   open_loading("Verifying credentials...".to_string());

   match profile.decrypt_zero(&dir) {
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
