//! One handle for the 32-byte secrets the crate keeps behind a mutex.

use std::hash::{Hash, Hasher};
use std::sync::{Mutex, MutexGuard};

use secure_types::{SecureArray, Zeroize};

/// A 32-byte secret behind a mutex, with the comparisons every secret key type needs.
///
/// `SpendingKey`, `ViewingKey` and `RailgunDbKey` all wrap `Mutex<SecureArray<u8, 32>>` and need the
/// same `Clone`/`Eq`/`Ord`/`Hash`/`unlock` behaviour. Copied per type, that meant three poison
/// messages that could drift and three places to keep the wipe right; the comparisons here never
/// leave an un-zeroized copy behind, because they all go through [`SecretKey32::with_copy`].
pub(crate) struct SecretKey32(Mutex<SecureArray<u8, 32>>);

impl SecretKey32 {
   pub(crate) fn new(key: SecureArray<u8, 32>) -> Self {
      Self(Mutex::new(key))
   }

   /// The guard. A poisoned lock means a panic unwound while the secret was held, so the bytes are
   /// in a state nothing here can reason about: fail loud rather than hand them out.
   pub(crate) fn lock(&self) -> MutexGuard<'_, SecureArray<u8, 32>> {
      self.0.lock().expect("secret key mutex poisoned")
   }

   pub(crate) fn unlock<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
      self.lock().unlock(f)
   }

   /// Run `f` over a copy of the bytes and wipe the copy afterwards — the only way these
   /// comparisons touch the secret.
   fn with_copy<R>(&self, f: impl FnOnce(&[u8; 32]) -> R) -> R {
      let mut copy = self.lock().unlock(|bytes| {
         let mut arr = [0u8; 32];
         arr.copy_from_slice(bytes);
         arr
      });
      let out = f(&copy);
      copy.zeroize();
      out
   }
}

impl Clone for SecretKey32 {
   fn clone(&self) -> Self {
      let inner = self.lock().clone();
      Self(Mutex::new(inner))
   }
}

impl Eq for SecretKey32 {}

impl PartialEq for SecretKey32 {
   fn eq(&self, other: &Self) -> bool {
      self.with_copy(|a| other.with_copy(|b| a == b))
   }
}

impl Ord for SecretKey32 {
   fn cmp(&self, other: &Self) -> std::cmp::Ordering {
      self.with_copy(|a| other.with_copy(|b| a.cmp(b)))
   }
}

impl PartialOrd for SecretKey32 {
   fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
      Some(self.cmp(other))
   }
}

impl Hash for SecretKey32 {
   fn hash<H: Hasher>(&self, state: &mut H) {
      self.with_copy(|bytes| state.write(bytes))
   }
}
