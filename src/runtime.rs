//! The process-wide tokio runtime, owned by `main`.
//!
//! Debug codegen keeps huge stack frames (async state machines, revm, Railgun
//! circuits), so workers get 8 MiB of stack — virtual until used.
use std::sync::OnceLock;
use tokio::runtime::{Builder, Handle, Runtime};

static HANDLE: OnceLock<Handle> = OnceLock::new();

pub fn build() -> Runtime {
   Builder::new_multi_thread()
      .enable_all()
      .thread_stack_size(8 * 1024 * 1024)
      .build()
      .expect("failed to build tokio runtime")
}

/// Publish the handle `main` owns. Called once, before any work is spawned.
pub fn init(rt: &Runtime) {
   let _ = HANDLE.set(rt.handle().clone());
}

/// The handle every background task in the binary spawns on.
pub fn rt() -> &'static Handle {
   // A test constructs UI values (`GUI::default`) without a `main` to install the runtime;
   // bootstrapping the test runtime here keeps those constructors on the same spawn path.
   #[cfg(test)]
   if HANDLE.get().is_none() {
      let _ = test_handle();
   }
   HANDLE.get().expect("runtime not initialised")
}

#[cfg(test)]
pub fn test_handle() -> Handle {
   static TEST_RUNTIME: OnceLock<Runtime> = OnceLock::new();
   let runtime = TEST_RUNTIME.get_or_init(build);
   // A test that drives a spawn site directly expects the same handle production uses;
   // publishing it here keeps `rt()` valid without a `main`.
   let _ = HANDLE.set(runtime.handle().clone());
   runtime.handle().clone()
}
