use anyhow::Error as AnyhowError;

#[derive(Debug)]
pub enum SmartAccountError {
   Provider(AnyhowError),
}

impl std::fmt::Display for SmartAccountError {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      match self {
         Self::Provider(err) => write!(f, "provider error: {err}"),
      }
   }
}

impl std::error::Error for SmartAccountError {
   fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
      match self {
         // `anyhow::Error` does not implement `std::error::Error`, so the
         // source is reached through its own `as_ref()`.
         Self::Provider(err) => Some(err.as_ref()),
      }
   }
}

impl From<AnyhowError> for SmartAccountError {
   fn from(err: AnyhowError) -> Self {
      Self::Provider(err)
   }
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
