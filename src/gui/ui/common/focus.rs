//! Where the keyboard goes when a window opens, and when a picker closes.

use egui::{Id, Ui};

/// Put the keyboard back on the control a dialog was opened from.
///
/// A window cannot do this from its own `show`: on the frame a dialog closes, the page under it
/// still draws behind the dialog's modal layer, and egui clears the focus of a widget drawn in that
/// state — `Context::create_widget` calls `Memory::ignore_focus` for anything not allowed
/// interaction yet. A request made from inside the window is therefore wiped by the page's own
/// widgets, which draw after it. The frame hands the focus back at the end of the pass instead —
/// see `central_panel::show` — where nothing follows but the announcer.
pub fn hand_focus_back(ctx: &egui::Context, opener: &mut Option<Id>) {
   if let Some(id) = opener.take() {
      ctx.memory_mut(|memory| memory.request_focus(id));
   }
}

/// Ask for the keyboard on the first frame a window paints, and once only.
///
/// A shrink-wrapping window spends its first passes sizing its body: nothing on it is visible
/// yet and a request made there does not survive the pass. Asking again on every frame is the
/// other failure — the keyboard is pulled back from whatever the reader reached next — so the ask
/// is recorded here and spent on the first pass that paints.
#[derive(Default, Clone, Copy)]
pub struct DeferredFocus {
   pending: bool,
}

impl DeferredFocus {
   /// A window that opens on its first control: the ask is already outstanding.
   pub const fn requested() -> Self {
      Self { pending: true }
   }

   /// Ask for the keyboard when the window next paints.
   pub fn request(&mut self) {
      self.pending = true;
   }

   /// Withdraw an ask that has not been answered yet.
   pub fn cancel(&mut self) {
      self.pending = false;
   }

   /// Take the keyboard for `id` on the first pass that paints.
   pub fn grab(&mut self, ui: &Ui, id: Id) {
      if self.pending && !ui.is_sizing_pass() {
         ui.memory_mut(|memory| memory.request_focus(id));
         self.pending = false;
      }
   }
}

/// Where the keyboard goes when a picker closes: back to the control it was opened from.
///
/// A dialog takes the keyboard while it is up, so the control that opened it loses it — and a pick
/// closes the picker from inside its own body, which is a close no dialog hands the focus back from
/// on its own. The opener is recorded once per opening, and given the focus back on the first frame
/// the dialog layer no longer covers: the layer outlives the close by a frame, and every widget
/// drawn behind it withdraws a focus it is handed.
#[derive(Default, Clone, Copy)]
pub enum FocusHome {
   /// Nothing recorded for this opening yet.
   #[default]
   Unrecorded,
   /// What had the keyboard when the picker opened, if anything did.
   Opened(Option<Id>),
   /// The picker has closed; this control takes the keyboard back on the next frame.
   Closing(Id),
}

impl FocusHome {
   /// Record the control a picker is opened from, when the opener knows its own id.
   ///
   /// [`Self::note`] reads the keyboard on the picker's first frame, which serves an opener that
   /// keeps its focus. A text field gives its focus up on the key that opens the picker, so that
   /// frame has nothing to read and the field's id has to be handed over instead.
   pub fn open_from(&mut self, opener: Id) {
      *self = Self::Opened(Some(opener));
   }

   /// Note the control a picker is opened from. Only the first note of an opening counts: the dialog
   /// takes the keyboard later in that same frame, and that must not become the home.
   pub fn note(&mut self, ui: &Ui) {
      if matches!(*self, Self::Unrecorded) {
         *self = Self::Opened(ui.memory(|memory| memory.focused()));
      }
   }

   /// The picker is closing: its opener takes the keyboard back once the dialog layer has gone.
   pub fn close(&mut self) {
      *self = match *self {
         Self::Opened(Some(id)) => Self::Closing(id),
         _ => Self::Unrecorded,
      };
   }

   /// The control that takes the keyboard back now, once the dialog layer has gone.
   ///
   /// The layer outlives the frame a picker closes on — it is still in force on the frame after —
   /// and every widget drawn behind it withdraws a focus it is handed, so the hand-back waits for
   /// it to clear instead of racing it.
   pub fn take_closing(&mut self, ui: &Ui) -> Option<Id> {
      match *self {
         Self::Closing(id) if ui.memory(|memory| memory.allows_interaction(ui.layer_id())) => {
            *self = Self::Unrecorded;
            Some(id)
         }
         _ => None,
      }
   }
}
