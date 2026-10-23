use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::{Arc, Mutex, RwLock};

/// A store held behind an [`Arc<RwLock>`], with the lock kept private.
///
/// Cloning is cheap and yields another handle to the same store. The guard is
/// deliberately never handed out: access goes through [`read`](Self::read) and
/// [`write`](Self::write).
pub struct RwLockHandle<T>(Arc<RwLock<T>>);

impl<T> Clone for RwLockHandle<T> {
   fn clone(&self) -> Self {
      Self(Arc::clone(&self.0))
   }
}

impl<T> RwLockHandle<T> {
   pub fn new(inner: T) -> Self {
      Self(Arc::new(RwLock::new(inner)))
   }

   /// Shared access to the guarded store.
   ///
   /// This is the poison policy shared by every zeus-core store lock: a
   /// poisoned store lock means a writer panicked mid-update and the guarded
   /// value is half-updated, so it is not handed out. The panic message names
   /// that condition instead of restating the error.
   pub fn read<R>(&self, f: impl FnOnce(&T) -> R) -> R {
      f(&self.0.read().expect("a zeus-core store lock was poisoned"))
   }

   /// Exclusive mutable access to the guarded store.
   ///
   /// Shares the poison policy documented on [`read`](Self::read).
   pub fn write<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
      f(&mut self.0.write().expect("a zeus-core store lock was poisoned"))
   }
}

impl<T: Serialize> Serialize for RwLockHandle<T> {
   fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
   where
      S: Serializer,
   {
      self.read(|inner| inner.serialize(serializer))
   }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for RwLockHandle<T> {
   fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
   where
      D: Deserializer<'de>,
   {
      let inner = T::deserialize(deserializer)?;
      Ok(Self::new(inner))
   }
}

/// A store held behind an [`Arc<Mutex>`], with the lock kept private.
///
/// Cloning is cheap and yields another handle to the same store. The guard is
/// deliberately never handed out: access goes through [`read`](Self::read) and
/// [`write`](Self::write).
pub struct MutexHandle<T>(Arc<Mutex<T>>);

impl<T> Clone for MutexHandle<T> {
   fn clone(&self) -> Self {
      Self(Arc::clone(&self.0))
   }
}

impl<T> MutexHandle<T> {
   pub fn new(inner: T) -> Self {
      Self(Arc::new(Mutex::new(inner)))
   }

   /// Shared access to the guarded store.
   ///
   /// This is the poison policy shared by every zeus-core store lock: a
   /// poisoned store lock means a writer panicked mid-update and the guarded
   /// value is half-updated, so it is not handed out. The panic message names
   /// that condition instead of restating the error.
   pub fn read<R>(&self, f: impl FnOnce(&T) -> R) -> R {
      f(&self.0.lock().expect("a zeus-core store lock was poisoned"))
   }

   /// Exclusive mutable access to the guarded store.
   ///
   /// Shares the poison policy documented on [`read`](Self::read).
   pub fn write<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
      f(&mut self.0.lock().expect("a zeus-core store lock was poisoned"))
   }
}

impl<T: Serialize> Serialize for MutexHandle<T> {
   fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
   where
      S: Serializer,
   {
      self.read(|inner| inner.serialize(serializer))
   }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for MutexHandle<T> {
   fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
   where
      D: Deserializer<'de>,
   {
      let inner = T::deserialize(deserializer)?;
      Ok(Self::new(inner))
   }
}
