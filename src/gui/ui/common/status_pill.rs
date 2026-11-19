//! The top panel's status pill, spoken.
//!
//! The pill is an `Area` with nothing to land on, so a reader hears nothing while work runs.
//! Announcing it every frame would repeat the same sentence, so only its transitions are
//! spoken: the message when the pill appears, a different message when the work changes, and
//! that Zeus is ready when the pill goes.

/// What a reader hears when the pill disappears: the work it was reporting is over.
pub const READY: &str = "Zeus is ready.";

/// The sentence to announce for a pill whose message changed between two frames.
///
/// `None` when there is nothing to say: no pill before and none now, or the same message still
/// running. A pill that changes from one message straight to another announces the new one only,
/// which is the sentence it is showing.
pub fn announcement(previous: Option<&str>, current: Option<&str>) -> Option<String> {
   match (previous, current) {
      (None, None) => None,
      (Some(previous), Some(current)) if previous == current => None,
      (Some(_), None) => Some(READY.to_string()),
      (None, Some(current)) | (Some(_), Some(current)) => Some(current.to_string()),
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   /// The pill appearing is the announcement of its message: the messages are full sentences, so
   /// the first frame that draws one is enough for a reader to hear what is running.
   #[test]
   fn a_pill_that_appears_announces_its_message() {
      assert_eq!(
         announcement(
            None,
            Some("Saving vault in progress, do not close Zeus yet!")
         ),
         Some("Saving vault in progress, do not close Zeus yet!".to_string())
      );
   }

   /// The pill going away means the work it reported is done, so that is what a reader hears
   /// rather than silence.
   #[test]
   fn a_pill_that_disappears_announces_ready() {
      assert_eq!(
         announcement(Some("Loading Railgun DB"), None),
         Some("Zeus is ready.".to_string())
      );
   }

   /// The same message two frames in a row is one announcement, not one per frame.
   #[test]
   fn an_unchanged_pill_says_nothing() {
      assert_eq!(announcement(None, None), None);
      assert_eq!(
         announcement(
            Some("Loading Railgun DB"),
            Some("Loading Railgun DB")
         ),
         None
      );
   }

   /// One message replacing another announces only the new sentence: the old work is over, and the
   /// pill now reports the new one.
   #[test]
   fn a_new_message_replaces_the_old_announcement() {
      assert_eq!(
         announcement(
            Some("Loading Railgun DB"),
            Some("Zeus is still syncing important data")
         ),
         Some("Zeus is still syncing important data".to_string())
      );
   }
}
