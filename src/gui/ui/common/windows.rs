//! Windows that are used throughout the app
//!
//! - ConfirmWindow - A Window to prompt the user to confirm an action
//! - UpdateWindow - Window to prompt the user to update Zeus version
//! - LoadingWindow - Window to indicate a loading state
//! - MsgWindow - Simple window diplaying a message, for example an error

use super::delayed_action_label;
use crate::gui::{GUI, SHARED_GUI};
use crate::utils::{
   TimeStamp,
   self_update::{UpdateInfo, restart_app, update_zeus},
};
use eframe::egui::{Align2, Color32, RichText, Spinner, Ui, Vec2, vec2};
use egui::Order;
use std::time::{Duration, Instant};

use egui_elements::{Button, Lightning, LightningSettings, Modal, ParticleLayer, Theme};

/// The single-prompt gate shared by the authorising windows.
///
/// A flow claims the gate before raising its prompt and releases it once the answer is collected;
/// `claim` refusing is what keeps two authorising prompts from being open at once. Every window that
/// can block on the user shares this one implementation, so the policy cannot drift between them.
#[derive(Default)]
pub struct PromptGate {
   pending: bool,
}

impl PromptGate {
   /// Takes the gate for one flow. `false` means another flow already holds it.
   pub fn claim(&mut self) -> bool {
      if self.pending {
         return false;
      }
      self.pending = true;
      true
   }

   /// Gives the gate back once the flow is done with the window.
   pub fn release(&mut self) {
      self.pending = false;
   }

   /// Whether a flow currently holds the gate.
   pub fn pending(&self) -> bool {
      self.pending
   }
}

/// A Window to prompt the user to confirm an action
pub struct ConfirmWindow {
   open: bool,
   /// A flow has claimed this window ([`Self::claim`]) and holds it until it is done with the
   /// prompt. Claimed under the same lock that opens it, so a second flow cannot replace the
   /// prompt the user is reading — several different actions share this one window.
   gate: PromptGate,
   pub confirm: Option<bool>,
   pub msg: String,
   pub msg2: Option<String>,
   /// When the prompt became visible. `None` skips the Confirm delay (in-app Zeus prompts).
   opened_at: Option<Instant>,
   pub size: (f32, f32),
}

impl ConfirmWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         gate: PromptGate::default(),
         confirm: None,
         msg: String::new(),
         msg2: None,
         opened_at: None,
         size: (450.0, 250.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   /// Open this prompt for one flow, claiming the window. Returns false — and changes nothing —
   /// when another flow already holds it.
   pub fn open(&mut self, msg: impl Into<String>) -> bool {
      self.open_inner(msg, false)
   }

   /// Dapp-originated prompt. Confirm is delayed so a focus-steal click cannot approve it, and the
   /// window is claimed so a second dapp request cannot replace what the user is reading.
   pub fn open_from_dapp(&mut self, msg: impl Into<String>) -> bool {
      self.open_inner(msg, true)
   }

   /// Claim the window and put up the prompt, or leave it untouched when another flow holds it.
   fn open_inner(&mut self, msg: impl Into<String>, delay: bool) -> bool {
      if self.is_busy() {
         return false;
      }

      let claimed = self.claim();

      self.open = true;
      self.msg = msg.into();
      self.msg2 = None;
      // Previous Confirm/Reject must not auto-approve the next prompt
      // (connect and switch share this window).
      self.confirm = None;
      self.opened_at = if delay { Some(Instant::now()) } else { None };
      claimed
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn set_msg2(&mut self, msg: impl Into<String>) {
      self.msg2 = Some(msg.into());
   }

   /// True while a confirmation is in flight: a flow holds the window's gate
   /// ([`PromptGate::pending`]) or the prompt is up.
   ///
   /// A flow refuses to raise a second prompt while this holds, so two flows can never share one
   /// window — and therefore one Confirm click.
   ///
   /// An answer still waiting in `confirm` is deliberately *not* busy. The flow that owns the window
   /// holds the claim until it has collected the answer, so an uncollected answer can only exist
   /// once that flow is gone — and reading it as busy would let a flow that died after the user
   /// answered wedge every later request.
   pub fn is_busy(&self) -> bool {
      self.gate.pending() || self.open
   }

   /// Claim the window for one flow, from just before its prompt is put up until it is done with
   /// it. Returns false when another flow already holds it.
   ///
   /// Taken inside [`Self::open`] / [`Self::open_from_dapp`], under the caller's lock, so the claim
   /// and the open have no gap. [`Self::release`] hands it back.
   fn claim(&mut self) -> bool {
      self.gate.claim()
   }

   pub fn release(&mut self) {
      self.gate.release()
   }

   /// Collect the answer, consuming it.
   ///
   /// Single-use on purpose: an answer that could be read twice would answer two flows from one
   /// Confirm click, and the second would act on a prompt the user never saw.
   pub fn take_confirm(&mut self) -> Option<bool> {
      self.confirm.take()
   }

   /// Collect the answer and close the prompt in one step, so one Confirm click
   /// can only ever be acted on once.
   pub fn take_answer(&mut self) -> Option<bool> {
      let confirmed = self.take_confirm();
      if confirmed.is_some() {
         self.close();
      }
      confirmed
   }

   /// Close and answer the awaiting prompt with a rejection. Used when
   /// auto-lock fires, so a confirm prompt cannot be approved while locked.
   pub fn reject(&mut self) {
      self.confirm = Some(false);
      self.close();
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let title = self.msg.clone();
      let mut open = self.open;

      let heading = RichText::new(&title).size(theme.typography.very_large);

      Modal::new(title, &mut open)
         .closable(false)
         .heading(heading)
         .header_separator(false)
         .center_header(true)
         // Same layers as the other authorising prompts (sign message, transaction
         // confirmation, app connect). It used to draw at `Tooltip`/`Debug`, a layer above
         // them, so whenever it was raised over a pending sign prompt it covered it. The
         // shared `authorising_prompt_busy` check now keeps two authorising prompts from being
         // up at once; this stops this window being *placed* above the others as well.
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .max_width(self.size.0)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = theme.spacing.md;
               ui.spacing_mut().item_spacing.x = theme.spacing.xl;
               ui.spacing_mut().button_padding = theme.button_padding;

               if let Some(msg) = &self.msg2 {
                  ui.label(RichText::new(msg).size(theme.typography.large));
               }

               ui.add_space(10.0);

               let button_size = vec2(
                  (ui.available_width() - theme.spacing.xl) * 0.5,
                  45.0,
               );

               ui.horizontal(|ui| {
                  ui.spacing_mut().item_spacing.x = theme.spacing.xl;
                  let visuals = theme.button_visuals();
                  let (confirm_ready, confirm_label) =
                     delayed_action_label(self.opened_at, "Confirm");
                  if !confirm_ready {
                     ui.ctx().request_repaint_after(Duration::from_millis(100));
                  }
                  let button =
                     Button::new(RichText::new(confirm_label).size(theme.typography.normal))
                        .visuals(visuals)
                        .min_size(button_size);

                  if ui.add_enabled(confirm_ready, button).clicked() {
                     self.close();
                     self.confirm = Some(true);
                  }

                  let button = Button::new(RichText::new("Reject").size(theme.typography.normal))
                     .visuals(visuals)
                     .min_size(button_size);

                  if ui.add(button).clicked() {
                     self.close();
                     self.confirm = Some(false);
                  }
               });
            });
         });
   }
}

/// Releases the confirm window's claim when the flow holding it ends.
///
/// A drop guard rather than a call at each exit point, mirroring the transaction confirmation's:
/// `ConfirmWindow` is shared by a dapp's network switch, a dapp's add-token, the MEV-protection
/// fallback inside a send, and the in-app token/NFT delete prompts, so the flow that raised one
/// must hand the window back on every path — including a future dropped mid-prompt.
pub struct ConfirmWindowClaim;

impl Drop for ConfirmWindowClaim {
   fn drop(&mut self) {
      SHARED_GUI.write(|gui| gui.confirm_window.release());
   }
}

/// True while any authorising prompt — a sign-message request, a transaction confirmation, the
/// shared confirm prompt, or an app-connect request — is in flight.
///
/// Every opener consults this under the same `SHARED_GUI.write` lock it uses to open (or claim) its
/// own prompt, so the check and the open have no gap: at most one prompt can be up, and a second
/// request can never replace what the user is reading. It answers for *all four* windows, not only
/// the one a flow raises, which is what stops a dapp's `wallet_switchEthereumChain` (or add-token,
/// or MEV fallback) from putting its prompt up over a pending sign-message prompt — the two used to
/// share one window layer, so the newer one was drawn on top.
///
/// The one intended *sequence* is the MEV-protection fallback inside a send (`core::tx::send`): the
/// same send raises that prompt only **after** `confirm_tx` has returned, so the transaction
/// confirmation's claim (a `ConfirmClaim`, dropped on return) is already released and the window it
/// used is closed. The two prompts therefore never overlap, and the fallback needs no exemption
/// here — it consults this predicate like every other opener. A future change that raised it while
/// the transaction prompt were still up would be refused, not silently stacked.
pub fn authorising_prompt_busy(gui: &GUI) -> bool {
   gui.sign_msg_window.is_busy()
      || gui.tx_confirmation_window.is_busy()
      || gui.confirm_window.is_busy()
      || gui.connect_dapp_window.is_busy()
}

/// Window to prompt the user to update Zeus version
pub struct UpdateWindow {
   open: bool,
   info: UpdateInfo,
   update_completed: bool,
   auto_restart_failed: bool,
   restart_in: u64,
   pub size: (f32, f32),
}

impl UpdateWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         info: Default::default(),
         update_completed: false,
         auto_restart_failed: false,
         restart_in: 0,
         size: (400.0, 250.0),
      }
   }

   pub fn open(&mut self, info: UpdateInfo) {
      self.open = true;
      self.info = info;
   }

   pub fn update_completed(&mut self, timestamp: u64) {
      self.update_completed = true;
      self.restart_in = timestamp;
   }

   pub fn auto_restart_failed(&mut self) {
      self.auto_restart_failed = true;
      self.update_completed = false;
   }

   pub fn reset(&mut self) {
      self.open = false;
      self.info = Default::default();
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let mut open = self.open;

      Modal::new("update_zeus", &mut open)
         .closable(false)
         .backdrop_order(Order::Foreground)
         .content_order(Order::Tooltip)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_max_height(self.size.1);
            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.md);
               ui.spacing_mut().button_padding = theme.button_padding;

               if self.update_completed {
                  self.update_completed_ui(theme, ui);
                  return;
               }

               if self.auto_restart_failed {
                  self.auto_restart_failed_ui(theme, ui);
                  return;
               }

               // A newer release with no build for this platform is a real release the user cannot
               // install from here. It used to be reported as "up to date" (the asset name decided
               // which release was the newest), and once reported the button claimed the update
               // information was missing. Both are worse than saying what is true.
               let installable = self.info.download_url.is_some() && self.info.asset_name.is_some();
               let version = self.info.version.as_deref().unwrap_or("a newer release");

               let text = "A new version of Zeus is available!";
               ui.label(RichText::new(text).size(theme.typography.large));

               let text = if installable {
                  format!("Would you like to update to {version} now?")
               } else {
                  format!("Zeus {version} is out, but it has no build for your platform yet.")
               };
               ui.label(RichText::new(text).size(theme.typography.normal));

               let visuals = theme.button_visuals();

               let label = if installable {
                  "Update Now"
               } else {
                  "More Info"
               };
               let text = RichText::new(label).size(theme.typography.normal);
               let update_button = Button::new(text).visuals(visuals);

               let text = RichText::new("Later").size(theme.typography.normal);
               let later_button = Button::new(text).visuals(visuals);

               let size = vec2(ui.available_width() * 0.45, 25.0);
               ui.allocate_ui(size, |ui| {
                  ui.horizontal(|ui| {
                     if ui.add(update_button).clicked() {
                        let info = self.info.clone();

                        crate::runtime::rt().spawn(async move {
                           if info.download_url.is_none() || info.asset_name.is_none() {
                              // The window already says it; this is the detail, with somewhere to go.
                              let version = info.version.as_deref().unwrap_or("a newer release");
                              let releases = crate::core::urls::ZeusUrl::ZeusReleases.base();

                              SHARED_GUI.write(|gui| {
                                 gui.loading_window.reset();
                                 gui.msg_window.open(format!(
                                    "Zeus {version} is newer than this build, but its release has \
                                     no build for your platform yet. You can see what it ships at \
                                     {releases}"
                                 ));
                              });
                              return;
                           }

                           SHARED_GUI.write(|gui| {
                              gui.loading_window.open("Download progress: 0%");
                           });

                           match update_zeus(
                              &info.download_url.unwrap(),
                              &info.asset_name.unwrap(),
                           )
                           .await
                           {
                              Ok(_) => {
                                 let now = TimeStamp::now_as_secs().unwrap_or_default().timestamp();
                                 let finish_on = now + 5;
                                 SHARED_GUI.write(|gui| {
                                    gui.loading_window.reset();
                                    gui.update_window.update_completed(finish_on);
                                    gui.request_repaint();
                                 });
                                 tracing::info!("Update successful!");
                              }
                              Err(e) => {
                                 SHARED_GUI.write(|gui| {
                                    gui.loading_window.reset();
                                    gui.msg_window.open(format!("Failed to update: {:?}", e));
                                    gui.request_repaint();
                                 });
                              }
                           }
                        });
                     }

                     if ui.add(later_button).clicked() {
                        self.reset();
                     }
                  });
               });
            });
         });
   }

   fn auto_restart_failed_ui(&mut self, theme: &Theme, ui: &mut Ui) {
      let text = RichText::new("Auto restart failed!").size(theme.typography.large);
      ui.label(text);

      let text = RichText::new("Please start Zeus manually").size(theme.typography.normal);
      ui.label(text);

      let visuals = theme.button_visuals();
      let text = RichText::new("Exit").size(theme.typography.normal);
      if ui.add(Button::new(text).visuals(visuals)).clicked() {
         std::process::exit(0);
      }
   }

   fn update_completed_ui(&mut self, theme: &Theme, ui: &mut Ui) {
      ui.add(Spinner::new().size(0.0).color(theme.colors.text));

      let text = RichText::new("Update completed!").size(theme.typography.large);
      ui.label(text);

      let current_unix = TimeStamp::now_as_secs().unwrap_or_default().timestamp();
      let restart_in = self.restart_in.saturating_sub(current_unix);

      let text = if restart_in == 0 {
         "Restarting now...".to_owned()
      } else {
         format!(
            "Restart in {} second{}",
            restart_in,
            if restart_in == 1 { "" } else { "s" }
         )
      };

      ui.label(RichText::new(text).size(theme.typography.normal));

      if restart_in == 0 {
         restart_app();
      }

      let visuals = theme.button_visuals();
      let text = RichText::new("Restart now").size(theme.typography.normal);
      if ui.add(Button::new(text).visuals(visuals)).clicked() {
         restart_app();
      }
   }
}

/// Window to indicate a loading state
pub struct LoadingWindow {
   open: bool,
   pub msg: String,
   pub size: (f32, f32),
   pub anchor: (Align2, Vec2),
}

/// Dedicated id for the loading window's strike.
///
/// The app-wide lightning has its own id; sharing one would make the two fight
/// over the same animation state in egui's temporary data.
const LOADING_STRIKE_ID: &str = "zeus_loading_strike";

/// The loading window's hardcoded strike — not user-configurable, and
/// independent of the ambient master switch.
fn loading_strike() -> LightningSettings {
   LightningSettings {
      open: false,
      active: true,
      color: Color32::from_rgb(190, 215, 255),
      bolt_count: 1,
      branch_depth: 2,
      thickness: 2.0,
      jitter: 0.18,
      flash_intensity: 0.35,
      duration_ms: 400.0,
      ambient_interval_secs: 0.5,
      layer: ParticleLayer::Top,
   }
}

impl LoadingWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         msg: String::new(),
         size: (200.0, 100.0),
         anchor: (Align2::CENTER_CENTER, vec2(0.0, 0.0)),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, msg: impl Into<String>) {
      self.open = true;
      self.msg = msg.into();
   }

   pub fn reset(&mut self) {
      self.open = false;
      self.msg = String::new();
      self.size = (200.0, 100.0);
   }

   pub fn new_size(&mut self, size: (f32, f32)) {
      self.size = size;
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      // Hardcoded strike while the window is up; it schedules its own next
      // strike, so an idle loading window requests no extra frames.
      Lightning::from_settings(LOADING_STRIKE_ID, &loading_strike()).show(ui);

      let mut open = self.open;

      Modal::new("Loading", &mut open)
         .backdrop_order(Order::Tooltip)
         .content_order(Order::Debug)
         .closable(false)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.vertical_centered(|ui| {
               ui.add(Spinner::new().size(50.0).color(theme.colors.text));
               ui.label(RichText::new(&self.msg).size(17.0));
            });
         });
   }
}

/// Simple window diplaying a message, for example an error
#[derive(Default)]
pub struct MsgWindow {
   open: bool,
   pub message: String,
   pub size: (f32, f32),
}

impl MsgWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         message: String::new(),
         size: (300.0, 300.0),
      }
   }

   /// Open the window with this title and message
   pub fn open(&mut self, msg: impl Into<String>) {
      self.open = true;
      self.message = msg.into();
   }

   pub fn reset(&mut self) {
      self.open = false;
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let msg = RichText::new(&self.message).size(theme.typography.normal);
      let mut open = self.open;

      Modal::new("msg_window", &mut open)
         .closable(false)
         .backdrop_order(Order::Tooltip)
         .content_order(Order::Debug)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_max_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = theme.spacing.xl;
               ui.spacing_mut().button_padding = theme.button_padding;

               ui.label(msg);

               let size = vec2(ui.available_width() * 0.5, 25.0);
               let text = RichText::new("OK").size(theme.typography.normal);
               let visuals = theme.button_visuals();
               let ok_button = Button::new(text).visuals(visuals).min_size(size);

               if ui.add(ok_button).clicked() {
                  self.reset();
               }
            });
         });
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   /// Auto-lock cancels a pending confirm: the window closes and the awaiting
   /// `wait_confirm_window` is answered "rejected" so it errors out.
   #[test]
   fn reject_answers_rejected_and_closes() {
      let mut window = ConfirmWindow::new();
      window.open("No available MEV protect RPC found");

      window.reject();

      assert!(!window.is_open());
      assert_eq!(window.take_confirm(), Some(false));
   }

   /// One Confirm click answers exactly one flow: the answer is collected once, so a second
   /// awaiting flow cannot read it and act on a prompt the user never saw.
   ///
   /// Several actions share this window — a dapp's network switch, a dapp's add-token, the
   /// MEV-protection fallback inside a send, and the in-app token/NFT delete prompts — so a page
   /// that fires two of them at once used to get one click's approval for both.
   #[test]
   fn a_confirm_stays_busy_until_its_answer_is_collected() {
      let mut window = ConfirmWindow::new();

      // Nothing in flight: a flow may claim the window.
      assert!(!window.is_busy());
      assert!(window.claim());
      assert!(window.is_busy());

      // The claim is exclusive for as long as the flow holds it — including while the prompt's
      // own state is still being set up and nothing is drawn yet.
      assert!(!window.claim());

      // The prompt is up.
      window.open = true;
      assert!(window.is_busy());

      // Answered, and the owning flow still holds the claim until it has collected it: still busy,
      // and the answer is collected once and only once.
      window.confirm = Some(true);
      window.close();
      assert!(window.is_busy());
      assert_eq!(window.take_confirm(), Some(true));
      assert_eq!(window.take_confirm(), None);

      // The owning flow releases the window when it is done with it.
      window.release();
      assert!(!window.is_busy());
   }

   /// The shared predicate answers for *every* authorising prompt, not only the one window a flow
   /// happens to raise. An opener consults it under the same lock it opens its own prompt with, so
   /// a dapp request can be refused because a signing prompt (or a connect prompt, or a transaction
   /// confirmation) is already waiting — the case where a switch/add-token/MEV prompt used to be
   /// drawn straight over the sign prompt the user was reading.
   #[test]
   fn shared_predicate_covers_every_authorising_prompt() {
      let mut gui = GUI::default();
      assert!(!authorising_prompt_busy(&gui));

      // Each window, held by a flow, alone makes the shared answer true.
      gui.confirm_window.claim();
      assert!(authorising_prompt_busy(&gui));
      gui.confirm_window.release();

      gui.connect_dapp_window.claim();
      assert!(authorising_prompt_busy(&gui));
      gui.connect_dapp_window.release();

      gui.sign_msg_window.claim();
      assert!(authorising_prompt_busy(&gui));
      gui.sign_msg_window.release();

      gui.tx_confirmation_window.claim();
      assert!(authorising_prompt_busy(&gui));
      gui.tx_confirmation_window.release();

      assert!(!authorising_prompt_busy(&gui));
   }

   /// The gate is exclusive: one flow holds it, a second claim is refused, and releasing hands it on.
   #[test]
   fn the_prompt_gate_hands_out_one_claim_at_a_time() {
      let mut gate = PromptGate::default();
      assert!(!gate.pending(), "a fresh gate is free");
      assert!(gate.claim(), "the first flow takes it");
      assert!(gate.pending());
      assert!(
         !gate.claim(),
         "a second flow is refused while the first holds it"
      );
      gate.release();
      assert!(!gate.pending(), "releasing frees it");
      assert!(gate.claim(), "and the next flow can take it");
   }

   /// The loading window's strike is hardcoded: these eight numbers are a
   /// deliberate look, so an edit that changes one should fail here and be
   /// re-confirmed with Mike.
   #[test]
   fn the_loading_strike_has_the_specified_shape() {
      let strike = loading_strike();

      assert_eq!(strike.bolt_count, 1);
      assert_eq!(strike.branch_depth, 2);
      assert_eq!(strike.thickness, 2.0);
      assert_eq!(strike.jitter, 0.18);
      assert_eq!(strike.flash_intensity, 0.35);
      assert_eq!(strike.duration_ms, 400.0);
      assert_eq!(strike.ambient_interval_secs, 0.5);
      assert_eq!(strike.color, Color32::from_rgb(190, 215, 255));

      assert!(strike.active);
      assert!(!strike.open);
      assert_eq!(strike.layer, ParticleLayer::Top);
      assert!(!LOADING_STRIKE_ID.is_empty());
   }
}
