//! The line that says why an action is blocked, and the sentence announced for it.

use egui::{Align, Layout, RichText, Ui, vec2};
use egui_elements::Theme;

/// Draw the sentence that says why an action is blocked, directly above its button.
///
/// A disabled button takes no focus, so a reader could not reach the reason while it lived in
/// the button's label — exactly when it mattered. The line is drawn in the button's own column,
/// so a long sentence wraps where the button ends, and nothing is drawn while nothing is
/// blocking. The Send form and the transaction confirmation share it, so both say a block the
/// same way.
pub fn blocked_reason_line(ui: &mut Ui, theme: &Theme, reason: &str) {
   let width = ui.available_width() * 0.8;
   let text = RichText::new(reason).size(theme.typography.normal).color(theme.colors.error);

   ui.allocate_ui_with_layout(
      vec2(width, 0.0),
      Layout::top_down(Align::Center),
      |ui| {
         ui.label(text);
      },
   );
}

/// The sentence to announce for a blocked action whose reason changed between two frames.
///
/// `None` when there is nothing to say: no reason before and none now, or the same reason still
/// blocking. A reason that goes away says nothing either — an enabled button is its own news, and
/// announcing the loss of a sentence nobody asked about would talk over what the user is doing.
pub fn blocked_announcement(previous: Option<&str>, current: Option<&str>) -> Option<String> {
   match (previous, current) {
      (None, None) => None,
      (Some(previous), Some(current)) if previous == current => None,
      (Some(_), None) => None,
      (None, Some(current)) | (Some(_), Some(current)) => Some(current.to_string()),
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   /// The rule the Send form follows: a reason that appears or changes is a new sentence, the same
   /// reason a frame later is not, and a block that lifts says nothing.
   #[test]
   fn a_blocked_reason_is_said_when_it_changes() {
      assert_eq!(blocked_announcement(None, None), None);

      assert_eq!(
         blocked_announcement(None, Some("Amount is not valid")).as_deref(),
         Some("Amount is not valid")
      );

      assert_eq!(
         blocked_announcement(
            Some("Amount is not valid"),
            Some("Amount is not valid")
         ),
         None
      );

      assert_eq!(
         blocked_announcement(
            Some("Amount is not valid"),
            Some("Insufficient ETH Balance")
         )
         .as_deref(),
         Some("Insufficient ETH Balance")
      );

      assert_eq!(
         blocked_announcement(Some("Amount is not valid"), None),
         None
      );
   }
}
