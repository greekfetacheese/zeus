//! The words a screen reader gets for an address, where Zeus draws one shortened.
//!
//! Zeus draws an address as six characters from each end joined by an ellipsis. That form cannot
//! tell two addresses apart when they agree on both ends, and the ellipsis drops the EIP-55
//! capitalisation, which is the only self-checking part of an EVM address. A 0zk address is worse
//! off: it carries no checksum at all, so a shortened one can neither be told apart nor verified.
//!
//! So a reader is given the same words the row draws: the name Zeus has for the address when it has
//! one, otherwise the shortening. A name the row does not say is heard twice over the glyphs it
//! draws, and the whole address spelled out is not something a listener can follow — forty hex
//! characters, one at a time, is a wall. The whole address stays as the node's description, for
//! whoever asks for it.

use crate::utils::truncate_address;
use egui::{Response, Ui};

use super::name_text;

/// What a screen reader should say for one address, and what it may add on request.
pub struct SpokenAddress {
   /// The accessible name: the words Zeus draws for this address — the name it has for it, or the
   /// shortening the row shows.
   pub name: String,
   /// The form read on request: the whole address, grouped so it can be followed.
   pub description: Option<String>,
}

/// The words for one address, named or not.
///
/// A name that is empty or only whitespace counts as no name: a failed Sourcify or ERC-7730 insert
/// stores one, and it would otherwise be read out in place of the address. A 0zk address with no
/// name is never read shortened — it says what it is, how long it is and how it ends, which is all
/// a listener can check by ear. Everything else unnamed is read as the shortening it is drawn as,
/// and its whole form waits in the description.
pub fn address_for_speech(name: Option<&str>, address: &str) -> SpokenAddress {
   let name = name.map(str::trim).filter(|name| !name.is_empty());

   // An address that is not there is silent: a sentence counting zero characters would be worse.
   if address.is_empty() {
      return SpokenAddress {
         name: name.unwrap_or_default().to_string(),
         description: None,
      };
   }

   if let Some(name) = name {
      return SpokenAddress {
         name: name.to_string(),
         description: Some(full_address(address)),
      };
   }

   if address.starts_with("0zk") {
      return SpokenAddress {
         name: format!(
            "Unknown 0zk address, {} characters, ending in {}",
            address.chars().count(),
            last_characters(address, 6),
         ),
         description: Some(address.to_string()),
      };
   }

   // The words on screen, so the row says one thing: the name a reader hears is the shortening the
   // label draws, and the address itself is what the description carries.
   SpokenAddress {
      name: truncate_address(address.to_string()),
      description: Some(full_address(address)),
   }
}

/// The sentence copying an address says: that it happened, and the form a listener can check.
///
/// An EVM address is read grouped, so the copy can be followed one group at a time; a 0zk address
/// has no checksum to check, so its ending is the one thing worth comparing.
pub fn copied_words(address: &str) -> String {
   match grouped_evm(address) {
      Some(grouped) => format!("Address copied: {grouped}"),
      None => format!(
         "Address copied, ending in {}",
         last_characters(address, 6)
      ),
   }
}

/// The name to draw for an address, and the words a screen reader gets for it.
///
/// One lookup answers both: a named address is drawn as its name, an unnamed one as the shortened
/// address, and the spoken name is that same string — the whole address waits in the description.
/// A name that is blank counts as no name here too, exactly as it does in [`address_for_speech`],
/// so a stored empty name cannot blank the row that drew the address.
pub fn drawn_and_spoken(name: Option<&str>, address: &str) -> (String, SpokenAddress) {
   let name = name.map(str::trim).filter(|name| !name.is_empty());
   let drawn = match name {
      Some(name) => name.to_string(),
      None => truncate_address(address.to_string()),
   };

   (drawn, address_for_speech(name, address))
}

/// The words for a drawn `from → to` line: two addresses and the arrow between them.
///
/// A transfer's sender and recipient are painted as one row whose parts are never given nodes of
/// their own, so the pair is read as one sentence rather than as two that do not exist.
pub fn transfer_for_speech(from: &SpokenAddress, to: &SpokenAddress) -> SpokenAddress {
   SpokenAddress {
      name: format!("{} to {}", from.name, to.name),
      description: Some(format!(
         "{} to {}",
         from.description.as_deref().unwrap_or(from.name.as_str()),
         to.description.as_deref().unwrap_or(to.name.as_str()),
      )),
   }
}

/// Put a spoken form on the node `response` already created for a drawn address.
///
/// The drawn text keeps its place on screen; the node is what a reader hears. The name goes
/// through [`name_text`], which is the one place a drawn label is named. What is left to this
/// function is the address's own second form: the whole address, for whoever asks for it.
pub fn name_address(ui: &Ui, response: &Response, spoken: &SpokenAddress) {
   name_text(ui, response, spoken.name.clone());

   if let Some(description) = &spoken.description {
      ui.ctx().accesskit_node_builder(response.id, |node| {
         node.set_description(description.as_str());
      });
   }
}

/// The form a reader gets on request: an EVM address in groups of four hex characters, so it can
/// be followed one group at a time. Anything else comes back exactly as it is.
fn full_address(address: &str) -> String {
   grouped_evm(address).unwrap_or_else(|| address.to_string())
}

/// `0x` followed by four hex characters per group — `0x12AB 34CD 56EF`.
///
/// `None` when grouping would not help: a string that is not `0x`-hex, or one with fewer than eight
/// hex characters, is handed back to the caller whole.
fn grouped_evm(address: &str) -> Option<String> {
   let hex = address.strip_prefix("0x")?;

   if hex.chars().count() < 8 || !hex.chars().all(|character| character.is_ascii_hexdigit()) {
      return None;
   }

   let mut grouped = String::with_capacity(address.len() + hex.chars().count() / 4);
   grouped.push_str("0x");

   for (index, character) in hex.chars().enumerate() {
      if index > 0 && index % 4 == 0 {
         grouped.push(' ');
      }

      grouped.push(character);
   }

   Some(grouped)
}

/// The last `taken` characters of `text`.
fn last_characters(text: &str, taken: usize) -> String {
   let characters: Vec<char> = text.chars().collect();
   let start = characters.len().saturating_sub(taken);

   characters[start..].iter().collect()
}

#[cfg(test)]
mod tests {
   use super::*;

   const VITALIK: &str = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045";

   /// An unnamed address is named by the shortening the row draws: forty hex characters, one at a
   /// time, is a spelling nobody can follow, and a name the glyphs below do not say is heard twice.
   /// The whole address waits in the description, so nothing is lost by saying the ends.
   #[test]
   fn an_unnamed_address_is_named_by_the_shortening_it_is_drawn_as() {
      let spoken = address_for_speech(None, VITALIK);

      assert_eq!(spoken.name, "0xd8dA...A96045");
      assert_eq!(
         spoken.name,
         truncate_address(VITALIK.to_string()),
         "the name is the text the row draws, so the row is read once"
      );
      assert_eq!(
         spoken.description.as_deref(),
         Some("0xd8dA 6BF2 6964 aF9D 7eEd 9e03 E534 15D3 7aA9 6045"),
         "the whole address is what a listener asks for"
      );
      assert_eq!(
         spoken.description.unwrap_or_default().replace(' ', ""),
         VITALIK,
         "grouping must not lose a character of the address"
      );
   }

   /// A named address is read as its name, with the full address kept for whoever asks for it —
   /// the name alone cannot be checked against anything.
   #[test]
   fn a_named_address_is_read_by_its_name() {
      let spoken = address_for_speech(Some("Ada's wallet"), VITALIK);

      assert_eq!(spoken.name, "Ada's wallet");
      assert_eq!(
         spoken.description.as_deref().map(|full| full.replace(' ', "")),
         Some(VITALIK.to_string())
      );
   }

   /// A stored name can be empty or whitespace (a failed Sourcify or ERC-7730 insert), and a name
   /// nobody can hear must not stand in for the address.
   #[test]
   fn a_blank_name_counts_as_no_name() {
      let unnamed = address_for_speech(None, VITALIK);

      for blank in ["", " ", "   ", "\t\n"] {
         let (drawn, spoken) = drawn_and_spoken(Some(blank), VITALIK);

         assert_eq!(
            drawn, unnamed.name,
            "a blank name ({blank:?}) must leave the row drawn as the address"
         );

         assert_eq!(
            spoken.name, unnamed.name,
            "a blank name ({blank:?}) is no name"
         );
         assert_eq!(spoken.description, unnamed.description);
      }
   }

   /// A 0zk address has no checksum, so its two ends are all a shortened form could ever say: the
   /// unnamed address is described instead of shortened, and nothing longer than six characters of
   /// it is read out.
   #[test]
   fn an_unnamed_0zk_address_is_never_read_shortened() {
      let address = format!("0zk{}", "a1B2c3D4e5".repeat(8));
      let spoken = address_for_speech(None, &address);

      let characters: Vec<char> = address.chars().collect();
      let ending: String = characters[characters.len() - 6..].iter().collect();

      assert!(spoken.name.starts_with("Unknown 0zk address, "));
      assert!(
         spoken.name.contains(&format!("{} characters", address.chars().count())),
         "the length is what a listener can use: {}",
         spoken.name
      );
      assert!(
         spoken.name.ends_with(&format!("ending in {ending}")),
         "the address ends the sentence: {}",
         spoken.name
      );

      for window in characters.windows(7) {
         let run: String = window.iter().collect();

         assert!(
            !spoken.name.contains(&run),
            "the name must not read a run of the address: {run}"
         );
      }
   }

   /// The price of a name a listener can follow: two addresses that agree on both ends are one
   /// string by ear. The whole of each is the description, which is where the two still differ —
   /// and the row is the one spelling instead of a wall of hex said twice.
   #[test]
   fn addresses_that_share_their_ends_sound_alike_and_differ_in_full() {
      let first = format!("0x1234{}ABCDEF", "1".repeat(30));
      let second = format!("0x1234{}ABCDEF", "2".repeat(30));

      assert_eq!(
         truncate_address(first.clone()),
         truncate_address(second.clone()),
         "the drawn form cannot tell them apart"
      );

      let first = address_for_speech(None, &first);
      let second = address_for_speech(None, &second);

      assert_eq!(
         first.name, second.name,
         "the ends are all a listener hears of an unnamed address"
      );
      assert_ne!(
         first.description, second.description,
         "the whole addresses must stay reachable, or the ends would be the only record"
      );
   }

   /// The callers hand over whatever a store or a decoded event carried, so a string with nothing
   /// to do with an address comes back as the row draws it: short enough to draw whole it is the
   /// string itself, and longer it is the shortening — never a form the row does not say.
   #[test]
   fn a_string_that_is_not_an_address_comes_back_as_it_is_drawn() {
      for text in ["0x1234", "0x1234567", "0x", "Ada Lovelace"] {
         assert_eq!(address_for_speech(None, text).name, text);
      }

      assert_eq!(
         address_for_speech(None, "No zkAddress").name,
         truncate_address("No zkAddress".to_string()),
         "a value long enough to be drawn shortened is named by that shortening"
      );

      assert_eq!(
         address_for_speech(Some("Ada"), "0x1234").name,
         "Ada"
      );
      assert_eq!(
         address_for_speech(None, "").name,
         "",
         "an address that is not there is silent, not a sentence about zero characters"
      );
   }

   /// A transfer line is one sentence, each side said the way its own rule says it: the name when it
   /// has one, the shortening when it does not. The whole of both sides is what the description
   /// carries, since neither end of a transfer can be checked from a shortening.
   #[test]
   fn a_from_to_line_is_one_sentence() {
      let from = address_for_speech(Some("Zeus"), VITALIK);
      let to = address_for_speech(None, VITALIK);
      let line = transfer_for_speech(&from, &to);

      assert_eq!(line.name, format!("Zeus to {}", to.name));
      assert_eq!(line.name, "Zeus to 0xd8dA...A96045");
      assert_eq!(
         line.description.as_deref(),
         Some(
            "0xd8dA 6BF2 6964 aF9D 7eEd 9e03 E534 15D3 7aA9 6045 to 0xd8dA 6BF2 6964 aF9D 7eEd 9e03 E534 15D3 7aA9 6045"
         ),
         "both whole addresses stay behind the sentence"
      );
   }

   /// A copy says which address landed on the clipboard. The drawn form cannot be checked by ear,
   /// so the sentence carries the grouped one — and a 0zk address, which has nothing to check,
   /// carries its ending rather than a sentence about how long it is.
   #[test]
   fn a_copy_says_which_address_it_copied() {
      assert_eq!(
         copied_words(VITALIK),
         "Address copied: 0xd8dA 6BF2 6964 aF9D 7eEd 9e03 E534 15D3 7aA9 6045"
      );

      let zk = format!("0zk{}", "a1B2c3D4e5".repeat(8));
      let characters: Vec<char> = zk.chars().collect();
      let ending: String = characters[characters.len() - 6..].iter().collect();

      assert_eq!(
         copied_words(&zk),
         format!("Address copied, ending in {ending}")
      );
   }
}
