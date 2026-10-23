use anyhow::Error as AnyhowError;

#[derive(Debug, thiserror::Error)]
pub enum SmartAccountError {
   #[error("provider error: {0}")]
   Provider(#[from] AnyhowError),
}

#[cfg(test)]
mod tests {
   use std::error::Error as _;

   use super::*;

   #[test]
   fn renders_the_pinned_message() {
      let err = SmartAccountError::Provider(anyhow::anyhow!("boom"));
      assert_eq!(err.to_string(), "provider error: boom");
   }

   #[test]
   fn the_source_is_the_anyhow_error() {
      let source = anyhow::anyhow!("boom");
      let rendered = source.to_string();

      let err = SmartAccountError::Provider(source);
      let found = err.source().expect("Provider carries a source");

      assert_eq!(found.to_string(), rendered);
   }
}
