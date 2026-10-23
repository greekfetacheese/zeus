use thiserror::Error;

#[derive(Debug, Error)]
pub enum BundlerError {
   #[error("Timeout")]
   Timeout,
   #[error("Other: {0}")]
   Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

#[cfg(test)]
mod tests {
   use std::error::Error as _;

   use super::*;

   #[test]
   fn renders_the_pinned_messages() {
      assert_eq!(BundlerError::Timeout.to_string(), "Timeout");
      assert_eq!(
         BundlerError::Other("boom".into()).to_string(),
         "Other: boom"
      );
   }

   #[test]
   fn only_the_other_variant_carries_a_source() {
      assert!(BundlerError::Timeout.source().is_none());
      assert_eq!(
         BundlerError::Other("boom".into()).source().map(ToString::to_string),
         Some("boom".to_owned())
      );
   }
}
