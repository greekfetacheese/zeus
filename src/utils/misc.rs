pub use crate::core::estimate_tx_cost;
pub use crate::core::platform::{TimeStamp, malloc_trim, truncate_address, truncate_hash};

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
