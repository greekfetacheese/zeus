//! A tokio handle for core's own unit tests.
//!
//! Core receives the process runtime through [`ZeusCtx`](crate::core::context::ZeusCtx); tests
//! build their own process-wide runtime here instead of reaching for the binary's.
use std::sync::OnceLock;
use tokio::runtime::{Builder, Handle, Runtime};

#[cfg(test)]
pub fn test_handle() -> Handle {
   static TEST_RUNTIME: OnceLock<Runtime> = OnceLock::new();
   TEST_RUNTIME
      .get_or_init(|| {
         Builder::new_multi_thread()
            .enable_all()
            .worker_threads(1)
            .build()
            .expect("failed to build the test tokio runtime")
      })
      .handle()
      .clone()
}
