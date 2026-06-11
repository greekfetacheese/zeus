use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use crate::gui::SHARED_GUI;
use crate::utils::{
   RT,
   self_update::{UpdateInfo, restart_app, update_zeus},
};
use eframe::egui::{Align2, Order, RichText, Sense, Spinner, Ui, Vec2, Window, vec2};
use std::{
   sync::Arc,
   time::{SystemTime, UNIX_EPOCH},
};
use zeus_wallet::Wallet;

use zeus_eth::types::ChainId;
use zeus_theme::{OverlayManager, Theme};
use zeus_widgets::{Button, ComboBox, Label};

pub mod dev;
pub mod sync;
pub mod tx_history;

/// A ComboBox to select a chain
pub struct ChainSelect {
   pub id: &'static str,
   pub chain: ChainId,
   pub size: Vec2,
   pub show_icon: bool,
   pub expansion: Option<f32>,
}

impl ChainSelect {
   pub fn new(id: &'static str, default_chain: u64) -> Self {
      Self {
         id,
         chain: ChainId::new(default_chain).unwrap(),
         size: (200.0, 25.0).into(),
         show_icon: true,
         expansion: Some(4.0),
      }
   }

   pub fn size(mut self, size: impl Into<Vec2>) -> Self {
      self.size = size.into();
      self
   }

   pub fn show_icon(mut self, show: bool) -> Self {
      self.show_icon = show;
      self
   }

   /// Show the ComboBox
   ///
   /// Returns true if the chain was changed
   pub fn show(
      &mut self,
      ignore_chain: u64,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) -> bool {
      let current_chain = self.chain;
      let mut clicked = false;
      let supported_chains = ChainId::supported_chains();
      let expansion = self.expansion;

      let text_size = theme.text_sizes.normal;
      let combo_visuals = theme.combo_box_visuals();
      let label_visuals = theme.label_visuals();
      let tint = theme.image_tint_recommended;
      let icon = icons.chain_icon(current_chain.id(), tint);

      let current_chain_label = Label::new(
         RichText::new(current_chain.name()).size(text_size),
         Some(icon),
      )
      .image_on_left()
      .sense(Sense::click())
      .visuals(label_visuals);

      ComboBox::new(self.id, current_chain_label)
         .visuals(combo_visuals)
         .width(self.size.x)
         .show_ui(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;

            for chain in supported_chains {
               if chain.id() == ignore_chain {
                  continue;
               }

               let text = RichText::new(chain.name()).size(text_size);
               let icon = icons.chain_icon(chain.id(), tint);

               let is_selected = chain == current_chain;
               let chain_label = Label::new(text.clone(), Some(icon))
                  .image_on_left()
                  .expand(expansion)
                  .fill_width(true)
                  .selected(is_selected)
                  .visuals(label_visuals)
                  .sense(Sense::click());

               if ui.add(chain_label).clicked() {
                  self.chain = chain;
                  clicked = true;
               }
            }
         });
      clicked
   }
}

/// A ComboBox to select a wallet
pub struct WalletSelect {
   pub id: &'static str,
   /// Selected Wallet
   pub wallet: Wallet,
   pub size: Vec2,
   pub expansion: Option<f32>,
}

impl WalletSelect {
   pub fn new(id: &'static str) -> Self {
      Self {
         id,
         wallet: Wallet::new_rng("I should not be here2".to_string()),
         size: (200.0, 25.0).into(),
         expansion: Some(6.0),
      }
   }

   pub fn size(mut self, size: impl Into<Vec2>) -> Self {
      self.size = size.into();
      self
   }

   pub fn expansion(mut self, expansion: f32) -> Self {
      self.expansion = Some(expansion);
      self
   }

   /// Show the ComboBox
   ///
   /// Returns true if the wallet was changed
   pub fn show(&mut self, theme: &Theme, ctx: ZeusCtx, icons: Arc<Icons>, ui: &mut Ui) -> bool {
      let mut clicked = false;
      let expansion = self.expansion;

      let combo_visuals = theme.combo_box_visuals();
      let label_visuals = theme.label_visuals();
      let wallet_icon = icons.wallet_main_x24();
      let text = RichText::new(&self.wallet.name_with_id_short()).size(theme.text_sizes.normal);

      let current_wallet_label = Label::new(text, Some(wallet_icon))
         .image_on_left()
         .expand(expansion)
         .visuals(label_visuals)
         .sense(Sense::click());

      ComboBox::new(self.id, current_wallet_label)
         .visuals(combo_visuals)
         .width(self.size.x)
         .show_ui(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 14.0;

            ctx.read(|ctx| {
               for wallet in ctx.vault_ref().all_wallets() {
                  let is_selected = wallet.address() == self.wallet.address();
                  let text =
                     RichText::new(wallet.name_with_id_short()).size(theme.text_sizes.normal);

                  let wallet_label = Label::new(text, None)
                     .fill_width(true)
                     .expand(expansion)
                     .selected(is_selected)
                     .visuals(label_visuals)
                     .sense(Sense::click());

                  if ui.add(wallet_label).clicked() {
                     self.wallet = wallet.clone();
                     clicked = true;
                  }
               }
            });
         });

      clicked
   }
}

/// A Window to prompt the user to confirm an action
pub struct ConfirmWindow {
   open: bool,
   overlay: OverlayManager,
   pub confirm: Option<bool>,
   pub msg: String,
   pub msg2: Option<String>,
   pub size: (f32, f32),
}

impl ConfirmWindow {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         confirm: None,
         msg: String::new(),
         msg2: None,
         size: (200.0, 100.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, msg: impl Into<String>) {
      if !self.open {
         self.overlay.window_opened();
      }
      self.open = true;
      self.msg = msg.into();
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   pub fn set_msg2(&mut self, msg: impl Into<String>) {
      self.msg2 = Some(msg.into());
   }

   pub fn get_confirm(&self) -> Option<bool> {
      self.confirm
   }

   pub fn reset(&mut self) {
      self.close();
      self.msg.clear();
      self.msg2 = None;
      self.confirm = None;
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let window_frame = theme.frame1;

      Window::new("confirm_window")
         .title_bar(false)
         .resizable(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .collapsible(false)
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 15.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               ui.label(RichText::new(&self.msg).size(theme.text_sizes.normal));

               if let Some(msg) = &self.msg2 {
                  ui.label(RichText::new(msg).size(theme.text_sizes.normal));
               }

               let visuals = theme.button_visuals();
               let button = Button::new(RichText::new("Confirm").size(theme.text_sizes.normal))
                  .visuals(visuals);

               if ui.add(button).clicked() {
                  self.close();
                  self.confirm = Some(true);
               }

               let button = Button::new(RichText::new("Reject").size(theme.text_sizes.normal))
                  .visuals(visuals);

               if ui.add(button).clicked() {
                  self.close();
                  self.confirm = Some(false);
               }
            });
         });
   }
}

/// Window to prompt the user to update Zeus version
pub struct UpdateWindow {
   open: bool,
   overlay: OverlayManager,
   info: UpdateInfo,
   update_completed: bool,
   auto_restart_failed: bool,
   restart_in: u64,
   pub size: (f32, f32),
}

impl UpdateWindow {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         info: Default::default(),
         update_completed: false,
         auto_restart_failed: false,
         restart_in: 0,
         size: (400.0, 150.0),
      }
   }

   pub fn open(&mut self, info: UpdateInfo) {
      if !self.open {
         self.overlay.window_opened();
      }
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
      self.overlay.window_closed();
      self.open = false;
      self.info = Default::default();
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let window_frame = theme.frame1;

      Window::new("Update Zeus")
         .title_bar(false)
         .resizable(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .collapsible(false)
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing = vec2(10.0, 15.0);
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               if self.update_completed {
                  self.update_completed_ui(theme, ui);
                  return;
               }

               if self.auto_restart_failed {
                  self.auto_restart_failed_ui(theme, ui);
                  return;
               }

               let text = "A new version of Zeus is available!";
               ui.label(RichText::new(text).size(theme.text_sizes.large));

               let text = "Would you like to update now?";
               ui.label(RichText::new(text).size(theme.text_sizes.normal));

               let visuals = theme.button_visuals();

               let text = RichText::new("Update Now").size(theme.text_sizes.normal);
               let update_button = Button::new(text).visuals(visuals);

               let text = RichText::new("Later").size(theme.text_sizes.normal);
               let later_button = Button::new(text).visuals(visuals);

               let size = vec2(ui.available_width() * 0.45, 25.0);
               ui.allocate_ui(size, |ui| {
                  ui.horizontal(|ui| {
                     if ui.add(update_button).clicked() {
                        let info = self.info.clone();

                        RT.spawn(async move {
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
                                 let now =
                                    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                                 let finish_on = now + 5;
                                 SHARED_GUI.write(|gui| {
                                    gui.loading_window.reset();
                                    gui.update_window.update_completed(finish_on);
                                 });
                                 tracing::info!("Update successful!");
                              }
                              Err(e) => {
                                 SHARED_GUI.write(|gui| {
                                    gui.loading_window.reset();
                                    gui.msg_window.open(
                                       "Update Error".to_string(),
                                       format!("Failed to update: {:?}", e),
                                    );
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
      let text = RichText::new("Auto restart failed!").size(theme.text_sizes.large);
      ui.label(text);

      let text = RichText::new("Please start Zeus manually").size(theme.text_sizes.normal);
      ui.label(text);

      let visuals = theme.button_visuals();
      let text = RichText::new("Exit").size(theme.text_sizes.normal);
      if ui.add(Button::new(text).visuals(visuals)).clicked() {
         std::process::exit(0);
      }
   }

   fn update_completed_ui(&mut self, theme: &Theme, ui: &mut Ui) {
      ui.add(Spinner::new().size(0.0).color(theme.colors.text));

      let text = RichText::new("Update completed!").size(theme.text_sizes.large);
      ui.label(text);

      let current_unix = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
      let restart_in = if current_unix < self.restart_in {
         self.restart_in - current_unix
      } else {
         0
      };

      let text = if restart_in == 0 {
         "Restarting now...".to_owned()
      } else {
         format!(
            "Restart in {} second{}",
            restart_in,
            if restart_in == 1 { "" } else { "s" }
         )
      };

      ui.label(RichText::new(text).size(theme.text_sizes.normal));

      if restart_in == 0 {
         restart_app();
      }

      let visuals = theme.button_visuals();
      let text = RichText::new("Restart now").size(theme.text_sizes.normal);
      if ui.add(Button::new(text).visuals(visuals)).clicked() {
         restart_app();
      }
   }
}

/// Window to indicate a loading state
pub struct LoadingWindow {
   open: bool,
   overlay: OverlayManager,
   pub msg: String,
   pub size: (f32, f32),
   pub anchor: (Align2, Vec2),
}

impl LoadingWindow {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         msg: String::new(),
         size: (200.0, 100.0),
         anchor: (Align2::CENTER_CENTER, vec2(0.0, 0.0)),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, msg: impl Into<String>) {
      if !self.open {
         self.overlay.window_opened();
      }
      self.open = true;
      self.msg = msg.into();
   }

   pub fn reset(&mut self) {
      self.overlay.window_closed();
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

      let window_frame = theme.frame1;

      Window::new("Loading")
         .title_bar(false)
         .order(Order::Debug)
         .resizable(false)
         .anchor(self.anchor.0, self.anchor.1)
         .collapsible(false)
         .frame(window_frame)
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
   overlay: OverlayManager,
   pub title: String,
   pub message: String,
   pub size: (f32, f32),
}

impl MsgWindow {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         title: String::new(),
         message: String::new(),
         size: (300.0, 100.0),
      }
   }

   /// Open the window with this title and message
   pub fn open(&mut self, title: impl Into<String>, msg: impl Into<String>) {
      if !self.open {
         self.overlay.window_opened();
      }
      self.open = true;
      self.title = title.into();
      self.message = msg.into();
   }

   pub fn reset(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let title = RichText::new(self.title.clone()).size(theme.text_sizes.large);
      let msg = RichText::new(&self.message).size(theme.text_sizes.normal);
      let window_frame = theme.frame1;

      Window::new(title)
         .resizable(false)
         .order(Order::Debug)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .collapsible(false)
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               ui.label(msg);

               let size = vec2(ui.available_width() * 0.5, 25.0);
               let text = RichText::new("OK").size(theme.text_sizes.normal);
               let visuals = theme.button_visuals();
               let ok_button = Button::new(text).visuals(visuals).min_size(size);

               if ui.add(ok_button).clicked() {
                  self.reset();
               }
            });
         });
   }
}
