use crate::gui::SHARED_GUI;

use lazy_static::lazy_static;
use std::time::Duration;
use tokio::runtime::{Builder, Runtime};

pub use crate::core::estimate_tx_cost;
pub use crate::core::platform::{TimeStamp, malloc_trim, truncate_address, truncate_hash};

lazy_static! {
   pub static ref RT: Runtime = build_runtime();
}

fn build_runtime() -> Runtime {
   // Tokio workers default to 2 MiB. Debug codegen keeps huge stack frames
   // (async state machines, revm, Railgun circuits), so those workers overflow.
   // 8 MiB matches the usual Linux pthread default and is virtual until used.
   Builder::new_multi_thread()
      .enable_all()
      .thread_stack_size(8 * 1024 * 1024)
      .build()
      .expect("failed to build tokio runtime")
}

pub async fn wait_tx_confirm() -> bool {
   loop {
      tokio::time::sleep(Duration::from_millis(50)).await;
      // Collect the answer and close in one exclusive step: the answer is
      // single-use, so one Confirm click can only ever be acted on once.
      let confirmed = SHARED_GUI.write(|gui| {
         let confirmed = gui.tx_confirmation_window.take_confirmed_or_rejected();
         if confirmed.is_some() {
            gui.tx_confirmation_window.close();
         }
         confirmed
      });
      if let Some(confirmed) = confirmed {
         return confirmed;
      }
   }
}

pub fn truncate_symbol_or_name(string: &str, max_chars: usize) -> String {
   if string.chars().count() > max_chars {
      // Take the first `max_chars` characters and collect them into a new String
      let truncated: String = string.chars().take(max_chars).collect();
      format!("{}...", truncated)
   } else {
      string.to_string()
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   /// A string too short to shorten is shown whole — the offsets are not safe until the length is known.
   ///
   /// The tail used to be a fixed `&address[36..]`, so anything under 36 bytes took the render path down
   /// with it: a truncated value out of a decoded event, a name, anything that is not a full address. A
   /// real address still shortens to its six-and-six form, which is what the callers expect.
   #[test]
   fn truncates_only_what_is_long_enough_to_truncate() {
      assert_eq!(truncate_address("0x1234".to_string()), "0x1234");
      assert_eq!(truncate_address(String::new()), "");
      assert_eq!(
         truncate_address("0x1234567890".to_string()),
         "0x1234567890",
         "twelve bytes is the shortest that stays whole"
      );

      let address = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045";
      assert_eq!(
         truncate_address(address.to_string()),
         "0xd8dA...A96045"
      );
   }

   /// The offsets are characters, not bytes.
   ///
   /// The input is whatever a caller has — a decoded event value, a name — so it is not guaranteed to
   /// be ASCII, and a byte offset lands inside the emoji here (its bytes are 4..8). That took the
   /// render path down with a panic instead of shortening the label.
   #[test]
   fn truncates_on_character_boundaries() {
      let label = "abcd😀aaaaaaaa".to_string();
      assert_eq!(label.len(), 16, "16 bytes");
      assert_eq!(label.chars().count(), 13, "13 characters");

      assert_eq!(truncate_address(label), "abcd😀a...aaaaaa");
   }
}
