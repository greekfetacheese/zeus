use thiserror::Error;

#[derive(Debug, Error)]
pub enum BundlerError {
   #[error("Timeout")]
   Timeout,
   #[error("Other: {0}")]
   Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}
