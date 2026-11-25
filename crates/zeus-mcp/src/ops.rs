//! The port the binary implements.
//!
//! The flows an agent must be able to start — a send, a swap, a bridge, a shield, an approval revoke —
//! are private functions in the binary crate, which a library cannot call. Rather than move the money
//! path into a crate, the binary installs an implementation of this trait, the same way the GUI
//! installs a [`zeus_core::ui::UiHost`] for the opposite direction.
//!
//! The trait is deliberately small in this slice: it carries what only the process knows about itself.
//! The flow methods arrive with the tools that need them, each with the arguments its own flow takes,
//! so this file never grows a speculative surface.

/// What the edge cannot know about the process it lives in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppInfo {
   /// The build's own version, as the about screen shows it.
   pub version: String,
}

/// Everything the binary has to supply.
///
/// A method that starts a flow returns a boxed future: `async fn` in a trait is not dyn-compatible, and
/// this trait is held as `Arc<dyn AgentOps>`. The one method here answers from memory, so it is a plain
/// function rather than a future around one.
pub trait AgentOps: Send + Sync + 'static {
   /// The running Zeus build.
   fn app_info(&self) -> AppInfo;
}

/// A double for tests and for a binary that has not installed anything yet.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoOps;

impl AgentOps for NoOps {
   fn app_info(&self) -> AppInfo {
      AppInfo {
         version: "0.0.0-test".to_owned(),
      }
   }
}
