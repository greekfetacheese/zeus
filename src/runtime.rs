//! The process-wide tokio runtime, owned by `main`.
//!
//! Debug codegen keeps huge stack frames (async state machines, revm, Railgun
//! circuits), so workers get 8 MiB of stack — virtual until used.
use std::sync::OnceLock;
use tokio::runtime::{Builder, Handle, Runtime};

static HANDLE: OnceLock<Handle> = OnceLock::new();

/// How long the runtime is given to finish in-flight blocking work at exit.
///
/// `Runtime::drop` waits for every blocking-pool task still running, and one of them
/// (`prefetch_railgun_circuits_if_allowed`) can be a multi-minute artifact download, so an
/// unbounded drop turns "Quit" into a hang with no window to show it. The vault save that
/// must finish is awaited before the close is allowed, so this only bounds what is left.
pub const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

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

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn shutdown_timeout_does_not_wait_for_a_running_blocking_task() {
      let rt = build();
      let handle = rt.handle().clone();

      // Prove the blocking task is actually running before the shutdown, otherwise the
      // pool has nothing to wait for and the measurement proves nothing.
      let (started_tx, started_rx) = std::sync::mpsc::channel();
      handle.spawn_blocking(move || {
         let _ = started_tx.send(());
         std::thread::sleep(std::time::Duration::from_secs(30));
      });
      started_rx
         .recv_timeout(std::time::Duration::from_secs(10))
         .expect("the blocking task must start before the shutdown");

      let begin = std::time::Instant::now();
      rt.shutdown_timeout(SHUTDOWN_GRACE);
      let elapsed = begin.elapsed();

      assert!(
         elapsed < std::time::Duration::from_secs(10),
         "shutdown waited {elapsed:?} for a blocking task; a plain drop would take 30 s"
      );
   }
}
