use crate::core::ZeusCtx;
use crate::gui::{
   self, SHARED_GUI,
   ui::{button, rich_text, text_edit_single},
};
use eframe::egui::{Frame, TextEdit, Ui, Vec2, Window, vec2};
use egui::{Align2, Color32};
use egui_theme::{
   Theme,
   utils::{bg_color_on_idle, border_on_hover, border_on_idle},
};
use ncrypt_me::zeroize::Zeroize;

pub struct AddWalletUi {
   pub open: bool,
   pub main_ui: bool,
   pub import_wallet: bool,
   pub generate_wallet: bool,
   pub imported_key: String,
   pub wallet_name: String,
   pub size: (f32, f32),
   pub anchor: (Align2, Vec2),
}

impl AddWalletUi {
   pub fn new(size: (f32, f32), offset: Vec2, align: Align2) -> Self {
      Self {
         open: false,
         main_ui: true,
         import_wallet: false,
         generate_wallet: false,
         imported_key: String::new(),
         wallet_name: String::new(),
         size,
         anchor: (align, offset),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      self.main_ui(theme, ui);
      self.import_wallet_ui(ctx.clone(), theme, ui);
      self.generate_wallet_ui(ctx.clone(), theme, ui);
   }

   pub fn main_ui(&mut self, theme: &Theme, ui: &mut Ui) {
      let mut open = self.main_ui;
      let mut clicked1 = false;
      let mut clicked2 = false;
      let frame = theme.frame3;

      Window::new("Add a new Wallet")
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               bg_color_on_idle(ui, Color32::TRANSPARENT);
               border_on_idle(ui, 1.0, Color32::LIGHT_GRAY);
               let size = vec2(ui.available_width() * 0.5, 50.0);

               // From private key
               let button1 = button(rich_text("From Private Key").heading())
                  .corner_radius(5)
                  .min_size(size);
               if ui.add(button1).clicked() {
                  clicked1 = true;
               }

               // Generate new wallet
               let button2 = button(rich_text("Generate New Wallet").heading())
                  .corner_radius(5)
                  .min_size(size);
               if ui.add(button2).clicked() {
                  clicked2 = true;
               }
            });
         });

      if clicked1 {
         self.import_wallet = true;
         open = false;
      }

      if clicked2 {
         self.generate_wallet = true;
         open = false;
      }

      self.main_ui = open;
   }

   pub fn import_wallet_ui(&mut self, ctx: ZeusCtx, _theme: &Theme, ui: &mut Ui) {
      let mut open = self.import_wallet;
      let mut clicked = false;
      Window::new("Import Wallet")
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;

               ui.scope(|ui| {
                  // border_on_idle(ui, 1.0, theme.colors.border_color_idle);
                  // border_on_hover(ui, 1.0, theme.colors.border_color_hover);

                  // Wallet Name
                  ui.label(rich_text("Wallet Name (Optional)"));
                  ui.add(TextEdit::singleline(&mut self.wallet_name).min_size(vec2(150.0, 25.0)));

                  // Private Key
                  ui.label(rich_text("Private Key"));
                  ui.add(
                     TextEdit::singleline(&mut self.imported_key)
                        .min_size(vec2(150.0, 25.0))
                        .password(true),
                  );
               });

               // Import Button
               let button = button(rich_text("Import"));
               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      if clicked {
         let name = self.wallet_name.clone();
         let key = self.imported_key.clone();

         std::thread::spawn(move || {
            let mut profile = ctx.profile();
            gui::utils::new_wallet_from_key(&mut profile, name, key);
            let dir = gui::utils::get_profile_dir();
            let info = gui::utils::get_encrypted_info(&dir);
            gui::utils::open_loading("Encrypting profile...".to_string());

            match profile.encrypt_and_save(&dir, info.argon2_params) {
               Ok(_) => {
                  let mut gui = SHARED_GUI.write().unwrap();
                  gui.wallet_ui.add_wallet_ui.imported_key.zeroize();
                  gui.wallet_ui.add_wallet_ui.wallet_name.clear();
                  gui.loading_window.open = false;
                  gui.open_msg_window("Wallet imported successfully", "");
                  ctx.write(|ctx| {
                     ctx.profile = profile;
                  })
               }
               Err(e) => {
                  let mut gui = SHARED_GUI.write().unwrap();
                  gui.wallet_ui.add_wallet_ui.imported_key.zeroize();
                  gui.loading_window.open = false;
                  gui.open_msg_window("Failed to save profile", e.to_string());
                  return;
               }
            }
         });
      }

      self.import_wallet = open;
      if !self.import_wallet {
         self.imported_key.zeroize();
      }
   }

   pub fn generate_wallet_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.generate_wallet;
      let mut clicked = false;
      Window::new("Generate Wallet")
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;

               ui.scope(|ui| {
                  border_on_idle(ui, 1.0, theme.colors.border_color_idle);
                  border_on_hover(ui, 1.0, theme.colors.border_color_hover);

                  // Wallet Name
                  ui.label(rich_text("Wallet Name (Optional)"));
                  ui.add(text_edit_single(&mut self.wallet_name));
               });

               // Generate Button
               let button = button(rich_text("Generate"));
               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      if clicked {
         let name = self.wallet_name.clone();

         std::thread::spawn(move || {
            let mut profile = ctx.profile();
            gui::utils::new_wallet_rng(&mut profile, name);
            let dir = gui::utils::get_profile_dir();
            let info = gui::utils::get_encrypted_info(&dir);
            gui::utils::open_loading("Encrypting profile...".to_string());

            match profile.encrypt_and_save(&dir, info.argon2_params) {
               Ok(_) => {
                  let mut gui = SHARED_GUI.write().unwrap();
                  gui.wallet_ui.add_wallet_ui.wallet_name.clear();
                  gui.loading_window.open = false;
                  gui.open_msg_window("Wallet generated successfully", "");
                  ctx.write(|ctx| {
                     ctx.profile = profile;
                  });
               }
               Err(e) => {
                  let mut gui = SHARED_GUI.write().unwrap();
                  gui.loading_window.open = false;
                  gui.open_msg_window("Failed to save profile", e.to_string());
                  return;
               }
            }
         });
      }
      self.generate_wallet = open;
   }
}
