use super::error::Bip32Error;
use std::{
   convert::TryFrom,
   iter::{FromIterator, IntoIterator},
   slice::Iter,
   str::FromStr,
};

pub const DEFAULT_DERIVATION_PATH_PREFIX: &str = "m/44'/60'/0'/0/";
pub const DEFAULT_DERIVATION_PATH: &str = "m/44'/60'/0'/0/0";

/// The hardened derivation flag. Keys at or above this index are hardened.
pub const BIP32_HARDEN: u32 = 0x8000_0000;

fn try_parse_index(s: &str) -> Result<u32, Bip32Error> {
   let mut index_str = s.to_owned();
   let harden = if s.ends_with('\'') || s.ends_with('h') {
      index_str.pop();
      true
   } else {
      false
   };

   index_str
      .parse::<u32>()
      .map(|v| if harden { harden_index(v) } else { v })
      .map_err(|_| Bip32Error::MalformattedDerivation(s.to_owned()))
}

fn encode_index(idx: u32, harden: char) -> String {
   let mut s = (idx % BIP32_HARDEN).to_string();
   if idx >= BIP32_HARDEN {
      s.push(harden);
   }
   s
}

/// Converts an raw index to hardened
pub const fn harden_index(index: u32) -> u32 {
   index + BIP32_HARDEN
}

/// A Bip32 derivation path
#[derive(Default, Debug, Clone, Eq, PartialEq)]
pub struct DerivationPath(Vec<u32>);

impl serde::Serialize for DerivationPath {
   fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
   where
      S: serde::Serializer,
   {
      serializer.serialize_str(&self.derivation_string())
   }
}

impl<'de> serde::Deserialize<'de> for DerivationPath {
   fn deserialize<D>(deserializer: D) -> Result<DerivationPath, D::Error>
   where
      D: serde::Deserializer<'de>,
   {
      let s: &str = serde::Deserialize::deserialize(deserializer)?;
      s.parse::<DerivationPath>().map_err(|e| serde::de::Error::custom(e.to_string()))
   }
}

impl DerivationPath {
   pub fn custom_string(&self, root: &str, joiner: char, harden: char) -> String {
      std::iter::once(root.to_owned())
         .chain(self.0.iter().map(|s| encode_index(*s, harden)))
         .collect::<Vec<String>>()
         .join(&joiner.to_string())
   }

   /// Return the last index in the path. None if the path is the root.
   pub fn last(&self) -> Option<&u32> {
      self.0.last()
   }

   /// Converts the path to a standard bip32 string. e.g `"m/44'/0'/0/32"`.
   pub fn derivation_string(&self) -> String {
      self.custom_string("m", '/', '\'')
   }

   /// Returns `True` if there are no indices in the path
   pub fn is_empty(&self) -> bool {
      self.0.is_empty()
   }

   /// The number of derivations in the path
   pub fn len(&self) -> usize {
      self.0.len()
   }

   /// Make an iterator over the path indices
   pub fn iter<'a>(&'a self) -> Iter<'a, u32> {
      self.0.iter()
   }

   /// `true` if `other` is a prefix of `self`
   pub fn starts_with(&self, other: &Self) -> bool {
      self.0.starts_with(&other.0)
   }

   /// Remove a prefix from a derivation. Return a new DerivationPath without the prefix.
   /// This is useful for determining the path to rech some descendant from some ancestor.
   pub fn without_prefix(&self, prefix: &Self) -> Option<DerivationPath> {
      if !self.starts_with(prefix) {
         None
      } else {
         Some(self.0[prefix.len()..].to_vec().into())
      }
   }

   /// Convenience function for finding the last hardened derivation in a path.
   /// Returns the index and the element. If there is no hardened derivation, it
   /// will return (0, None).
   pub fn last_hardened(&self) -> (usize, Option<u32>) {
      match self.iter().rev().position(|v| *v >= BIP32_HARDEN) {
         Some(rev_pos) => {
            let pos = self.len() - rev_pos - 1;
            (pos, Some(self.0[pos]))
         }
         None => (0, None),
      }
   }

   /// Return a clone with a resized path. If the new size is shorter, this truncates it. If the
   /// new path is longer, we pad with the second argument.
   pub fn resized(&self, size: usize, pad_with: u32) -> Self {
      let mut child = self.clone();
      child.0.resize(size, pad_with);
      child
   }

   /// Append an additional derivation to the end, return a clone
   pub fn extended(&self, idx: u32) -> Self {
      let mut child = self.clone();
      child.0.push(idx);
      child
   }
}

impl From<&DerivationPath> for DerivationPath {
   fn from(v: &DerivationPath) -> Self {
      v.clone()
   }
}

impl From<Vec<u32>> for DerivationPath {
   fn from(v: Vec<u32>) -> Self {
      Self(v)
   }
}

impl From<&Vec<u32>> for DerivationPath {
   fn from(v: &Vec<u32>) -> Self {
      Self(v.clone())
   }
}

impl From<&[u32]> for DerivationPath {
   fn from(v: &[u32]) -> Self {
      Self(Vec::from(v))
   }
}

impl TryFrom<u32> for DerivationPath {
   type Error = Bip32Error;

   fn try_from(v: u32) -> Result<Self, Self::Error> {
      Ok(Self(vec![v]))
   }
}

impl TryFrom<&str> for DerivationPath {
   type Error = Bip32Error;

   fn try_from(v: &str) -> Result<Self, Self::Error> {
      v.parse()
   }
}

impl FromIterator<u32> for DerivationPath {
   fn from_iter<T>(iter: T) -> Self
   where
      T: IntoIterator<Item = u32>,
   {
      Vec::from_iter(iter).into()
   }
}

impl FromStr for DerivationPath {
   type Err = Bip32Error;

   fn from_str(s: &str) -> Result<Self, Self::Err> {
      s.split('/')
         .filter(|v| v != &"m")
         .map(try_parse_index)
         .collect::<Result<Vec<u32>, Bip32Error>>()
         .map(|v| v.into())
         .map_err(|_| Bip32Error::MalformattedDerivation(s.to_owned()))
   }
}
