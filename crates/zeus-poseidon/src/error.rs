//! # Error
//! Contains the Error messages from this crate.

use thiserror::Error;

/// An Error enum capturing the errors produced by this crate.
#[derive(Error, Debug)]
pub enum Error {
   /// The provided parameters are invalid
   #[error("The provided parameters are invalid")]
   InvalidParameters,
   #[error("Unsupported input length: {0}")]
   UnsupportedInputLength(usize),
   /// The provided string is not a field element
   #[error("The provided string is not a field element")]
   ParseString,
   #[error("Err: {0}")]
   Other(String),
}

impl From<String> for Error {
   fn from(mes: String) -> Self {
      Self::Other(mes)
   }
}
impl From<&str> for Error {
   fn from(mes: &str) -> Self {
      Self::Other(mes.to_owned())
   }
}

#[cfg(test)]
mod tests {
   use std::error::Error as _;

   use super::*;

   #[test]
   fn renders_the_pinned_messages() {
      assert_eq!(
         Error::InvalidParameters.to_string(),
         "The provided parameters are invalid"
      );
      assert_eq!(
         Error::UnsupportedInputLength(32).to_string(),
         "Unsupported input length: 32"
      );
      assert_eq!(
         Error::ParseString.to_string(),
         "The provided string is not a field element"
      );
      assert_eq!(
         Error::Other("boom".to_owned()).to_string(),
         "Err: boom"
      );
   }

   #[test]
   fn errors_carry_no_source() {
      assert!(Error::InvalidParameters.source().is_none());
      assert!(Error::UnsupportedInputLength(0).source().is_none());
      assert!(Error::ParseString.source().is_none());
      assert!(Error::Other("boom".to_owned()).source().is_none());
   }
}
