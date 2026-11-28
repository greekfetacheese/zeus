//! The rendezvous file, in a test binary of its own.
//!
//! Every server writes the same `data/mcp.json` and a stop removes it, so a test that asserts on the
//! file cannot share a process with tests that start and stop servers of their own — one of them would
//! delete the file out from under it. One binary, one server, one file.

use std::path::PathBuf;
use std::sync::Arc;
use zeus_core::ZeusCtx;
use zeus_mcp::{NoOps, ServerConfig, ServerHandle, ServerStatus};

/// A working directory of this binary's own, so `data/` is never the repository's.
fn workspace() -> PathBuf {
   let dir = std::env::temp_dir().join(format!(
      "zeus-mcp-session-file-{}",
      std::process::id()
   ));
   std::fs::create_dir_all(&dir).expect("a temporary working directory");
   std::env::set_current_dir(&dir).expect("move into the temporary working directory");
   dir
}

#[tokio::test]
async fn the_session_file_is_owner_only_and_gone_after_a_stop() {
   workspace();

   let ctx = ZeusCtx::new(tokio::runtime::Handle::current());

   let mut config = ServerConfig::new(
      ctx,
      Arc::new(NoOps),
      tokio::runtime::Handle::current(),
   );
   config.preferred_port = 0;

   let server = ServerHandle::start(config).expect("the endpoint binds");

   let path = zeus_core::persisted::file_path(zeus_core::persisted::PersistedFile::Mcp)
      .expect("the rendezvous path");

   let card: serde_json::Value =
      serde_json::from_slice(&std::fs::read(&path).expect("a written session file"))
         .expect("a JSON session file");

   assert_eq!(
      card.get("port").and_then(serde_json::Value::as_u64),
      Some(u64::from(server.port()))
   );

   assert_eq!(
      card.get("pid").and_then(serde_json::Value::as_u64),
      Some(u64::from(std::process::id()))
   );

   #[cfg(unix)]
   {
      use std::os::unix::fs::PermissionsExt;

      let mode = std::fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;

      assert_eq!(mode, 0o600, "the session file must be owner-only");
   }

   server.stop();

   assert_eq!(server.status(), ServerStatus::Stopped);
   assert!(
      !path.exists(),
      "the session file must be gone after a stop"
   );
}
