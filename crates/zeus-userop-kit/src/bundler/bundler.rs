#[derive(Debug)]
pub enum BundlerError {
   Timeout,
   Other(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for BundlerError {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      match self {
         Self::Timeout => f.write_str("Timeout"),
         Self::Other(err) => write!(f, "Other: {err}"),
      }
   }
}

impl std::error::Error for BundlerError {
   fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
      match self {
         Self::Timeout => None,
         Self::Other(err) => Some(&**err),
      }
   }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for BundlerError {
   fn from(err: Box<dyn std::error::Error + Send + Sync>) -> Self {
      Self::Other(err)
   }
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
