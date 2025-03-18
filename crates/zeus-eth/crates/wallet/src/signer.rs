use alloy_signer_local::PrivateKeySigner;
use core::fmt;
use secure_types::{Zeroize, SecureString};
use std::borrow::Borrow;
use memsec::{mlock, munlock};

/// Wrapper type around [PrivateKeySigner]
///
/// - Securely erases the key when it is dropped
/// - For `Windows` calls `VirtualLock` to protect the key from being swapped out to disk
/// - For `Unix` calls `mlock` to prevent the key from being swapped to disk and memory dumped
///
/// ### Note on `Windows` is not possible to prevent memory dumping
#[derive(PartialEq)]
pub struct SecureSigner {
   signer: PrivateKeySigner,
}

impl SecureSigner {
   pub fn new(mut signer: PrivateKeySigner) -> Self {
      unsafe {
         let ptr: *mut PrivateKeySigner = &mut signer;
         let ptr = ptr as *mut u8;
         mlock(ptr, std::mem::size_of::<PrivateKeySigner>());
      }
      SecureSigner { signer }
   }

   pub fn random() -> Self {
      Self::new(PrivateKeySigner::random())
   }

   /// Return the signer's key in a SecureString
   pub fn key_string(&self) -> SecureString {
      let mut key_vec = self.signer.to_bytes();
      let string = key_vec
         .iter()
         .map(|b| format!("{b:02x}"))
         .collect::<String>();
      key_vec.zeroize();
      SecureString::from(string)
   }

   /// Convert into a normal PrivateKeySigner
   ///
   /// This will consume the SecureSigner
   ///
   /// You are responsible for zeroizing the contents of the returned PrivateKeySigner
   pub fn into_signer(mut self) -> PrivateKeySigner {
      let signer = std::mem::replace(&mut self.signer, PrivateKeySigner::random());

      unsafe {
         let ptr: *mut PrivateKeySigner = &mut self.signer;
         let ptr = ptr as *mut u8;
         munlock(ptr, std::mem::size_of::<PrivateKeySigner>());
      }

      std::mem::forget(self);
      signer
   }

   pub fn borrow(&self) -> &PrivateKeySigner {
      self.signer.borrow()
   }

   /// Erase the signer's key from memory
   pub fn erase(&mut self) {
      unsafe {
         let ptr: *mut PrivateKeySigner = &mut self.signer;
         let bytes: &mut [u8] = std::slice::from_raw_parts_mut(ptr as *mut u8, std::mem::size_of::<PrivateKeySigner>());
         bytes.zeroize();
      }
   }

   pub fn is_erased(&self) -> bool {
      let mut key_bytes = self.signer.to_bytes();
      let erased = key_bytes.iter().all(|&b| b == 0);
      key_bytes.zeroize();
      erased
   }
}

impl Drop for SecureSigner {
   fn drop(&mut self) {
      self.erase();
      unsafe {
         let ptr: *mut PrivateKeySigner = &mut self.signer;
         let ptr = ptr as *mut u8;
         munlock(ptr, std::mem::size_of::<PrivateKeySigner>());
      }
   }
}

impl Clone for SecureSigner {
   fn clone(&self) -> Self {
      Self::new(self.signer.clone())
   }
}

impl Borrow<PrivateKeySigner> for SecureSigner {
   fn borrow(&self) -> &PrivateKeySigner {
      &self.signer
   }
}

impl fmt::Debug for SecureSigner {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      f.write_str("***SECRET***").map_err(|_| fmt::Error)
   }
}

impl fmt::Display for SecureSigner {
   fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
      f.write_str("***SECRET***").map_err(|_| fmt::Error)
   }
}

impl serde::Serialize for SecureSigner {
   fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
   where
      S: serde::Serializer,
   {
      let mut key_bytes = self.signer.to_bytes().to_vec();
      let result = serializer.serialize_bytes(&key_bytes);
      key_bytes.zeroize();
      result
   }
}

impl<'de> serde::Deserialize<'de> for SecureSigner {
   fn deserialize<D>(deserializer: D) -> Result<SecureSigner, D::Error>
   where
      D: serde::Deserializer<'de>,
   {
      struct SecureVisitor;
      impl<'de> serde::de::Visitor<'de> for SecureVisitor {
         type Value = SecureSigner;
         fn expecting(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
            write!(formatter, "a sequence of bytes")
         }
         fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
         where
            A: serde::de::SeqAccess<'de>,
         {
            let mut vec = Vec::new();
            while let Some(byte) = seq.next_element::<u8>()? {
               vec.push(byte);
            }
            let signer = if let Ok(signer) = PrivateKeySigner::from_slice(&vec) {
               vec.zeroize();
               signer
            } else {
               vec.zeroize();
               return Err(serde::de::Error::custom("invalid private key"));
            };
            Ok(SecureSigner::new(signer))
         }
      }
      deserializer.deserialize_seq(SecureVisitor)
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn test_key_string() {
      let signer = SecureSigner::random();
      let key_string = signer.key_string();
      assert_eq!(key_string.borrow().len(), 64);
   }

   #[test]
   fn test_into_signer() {
      let signer = SecureSigner::random();
      let signer = signer.into_signer();
      assert_eq!(signer.address().len(), 20);
      println!("Signer Address: {:?}", signer.address());
   }

   #[test]
   fn test_erase_signer() {
      let mut secure_signer = SecureSigner::random();
      let key = secure_signer.borrow().to_bytes();
      println!(
         "Signer key before the erase: {}",
         key.iter().map(|b| format!("{b:02x}")).collect::<String>()
      );

      secure_signer.erase();

      let key = secure_signer.borrow().to_bytes();

      println!(
         "Signer key after the erase: {}",
         key.iter().map(|b| format!("{b:02x}")).collect::<String>()
      );

      assert!(secure_signer.is_erased());
   }

   #[test]
   fn test_signer_serde() {
      let secure_signer = SecureSigner::random();
      let serialized = serde_json::to_string(&secure_signer).unwrap();
      let deserialized: SecureSigner = serde_json::from_str(&serialized).unwrap();
      assert_eq!(deserialized.key_string().borrow(), secure_signer.key_string().borrow());
   }
}
