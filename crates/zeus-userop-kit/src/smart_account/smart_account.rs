use anyhow::Error as AnyhowError;

#[derive(Debug, thiserror::Error)]
pub enum SmartAccountError {
   #[error("provider error: {0}")]
   Provider(#[from] AnyhowError),
}
