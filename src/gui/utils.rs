use std::path::PathBuf;
use crate::core::user::Profile;
use crate::gui::SHARED_GUI;
use ncrypt::EncryptedInfo;

// Shortcuts for functions that may fail but we need to show an error message
// These functions are only called on a seperate thread so we dont block or panic the main thread

pub fn import_wallet(profile: &mut Profile, name: String, key: String) {
    match profile.import_wallet(name, key) {
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

pub fn new_wallet(profile: &mut Profile, name: String) {
    match profile.new_wallet(name) {
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