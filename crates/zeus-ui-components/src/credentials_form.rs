use super::input_field::SecureInputField;
use super::virtual_keyboard::VirtualKeyboard;
use egui::*;
use zeus_theme::Theme;
use zeus_widgets::SecureString;

/// A credentials form that can be used to edit username, password, and optionally confirm password.
pub struct CredentialsForm {
   open: bool,
   confrim_password: bool,
   username: SecureInputField,
   password: SecureInputField,
   confirm_password: SecureInputField,
   y_spacing: f32,
   x_spacing: f32,
   last_focused: &'static str,
   virtual_keyboard: VirtualKeyboard,
}

impl CredentialsForm {
   /// Create a new credentials form.
   ///
   /// # Panics
   ///
   /// If the `SecureString` allocation fails.
   pub fn new() -> Self {
      let username = SecureInputField::new("Username", false, true);
      let password = SecureInputField::new("Password", true, true);
      let confirm_password = SecureInputField::new("Confirm Password", true, true);
      Self {
         open: false,
         confrim_password: false,
         username,
         password,
         confirm_password,
         y_spacing: 15.0,
         x_spacing: 10.0,
         last_focused: "Username",
         virtual_keyboard: VirtualKeyboard::new(false),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   /// Erase the credentials from memory by zeroizing the username and password fields.
   pub fn erase(&mut self) {
      self.username.erase();
      self.password.erase();
      self.confirm_password.erase();
   }

   /// Reset the state of the credentials form.
   pub fn reset(&mut self) {
      *self = Self::new();
   }

   pub fn is_confirm_password(&self) -> bool {
      self.confrim_password
   }

   /// Copy password to confirm password
   fn copy_passwd_to_confirm(&mut self) {
      let new_text = self.password.text.clone();
      self.confirm_password.set_text(new_text);
   }

   /// Return a clone of the username
   pub fn username(&self) -> SecureString {
      self.username.text.clone()
   }

   /// Return a clone of the password
   pub fn password(&self) -> SecureString {
      self.password.text.clone()
   }

   /// Return a clone of the confirm password
   pub fn confirm_password(&self) -> SecureString {
      self.confirm_password.text.clone()
   }

   pub fn with_open(mut self, open: bool) -> Self {
      self.open = open;
      self
   }

   /// Initialize with enabled virtual keyboard.
   pub fn with_enabled_virtual_keyboard(mut self) -> Self {
      self.virtual_keyboard.open();
      self
   }

   /// Enable the virtual keyboard.
   pub fn enable_virtual_keyboard(&mut self) {
      self.virtual_keyboard.open();
   }

   /// Disable the virtual keyboard.
   pub fn disable_virtual_keyboard(&mut self) {
      self.virtual_keyboard.close();
   }

   /// Set whether to enable the confirm password field.
   pub fn set_confirm_password(&mut self, confirm_password: bool) {
      self.confrim_password = confirm_password;
   }

   /// Set the minimum size of this input field
   pub fn set_min_size(&mut self, size: Vec2) {
      self.username.set_min_size(size);
      self.password.set_min_size(size);
      self.confirm_password.set_min_size(size);
   }

   /// Adjust the icon size.
   pub fn set_icon_size(&mut self, size: Vec2) {
      self.username.set_icon_size(size);
      self.password.set_icon_size(size);
      self.confirm_password.set_icon_size(size);
   }

   /// Set the username text
   pub fn set_username_text(&mut self, text: SecureString) {
      self.username.set_text(text);
   }

   /// Set the password text
   pub fn set_password_text(&mut self, text: SecureString) {
      self.password.set_text(text);
   }

   /// Set the confirm password text
   pub fn set_confirm_password_text(&mut self, text: SecureString) {
      self.confirm_password.set_text(text);
   }

   /// Adjust the y spacing between the input fields.
   pub fn with_y_spacing(mut self, y_spacing: f32) -> Self {
      self.y_spacing = y_spacing;
      self
   }

   /// Adjust the x spacing between the input fields.
   pub fn with_x_spacing(mut self, x_spacing: f32) -> Self {
      self.x_spacing = x_spacing;
      self
   }

   /// Whether to enable the confirm password field.
   pub fn with_confirm_password(mut self, confirm_password: bool) -> Self {
      self.confrim_password = confirm_password;
      self
   }

   pub fn with_min_size(mut self, size: Vec2) -> Self {
      let username = self.username.clone();
      let password = self.password.clone();
      let confirm_password = self.confirm_password.clone();

      let username = username.with_min_size(size);
      let password = password.with_min_size(size);
      let confirm_password = confirm_password.with_min_size(size);

      self.username = username;
      self.password = password;
      self.confirm_password = confirm_password;

      self
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      ui.vertical_centered(|ui| {
         ui.spacing_mut().item_spacing.y = self.y_spacing;
         ui.spacing_mut().item_spacing.x = self.x_spacing;

         let user_res = self.username.show(theme, ui);
         let pass_res = self.password.show(theme, ui);

         let confirm_password_focused = if self.confrim_password {
            let res = self.confirm_password.show(theme, ui);
            res.map(|res| res.response.gained_focus()).unwrap_or(false)
         } else {
            self.copy_passwd_to_confirm();
            false
         };

         let username_focused = user_res
            .map(|res| res.response.gained_focus())
            .unwrap_or(false);

         let password_focused = pass_res
            .map(|res| res.response.gained_focus())
            .unwrap_or(false);

         if username_focused {
            self.last_focused = "Username";
         }

         if password_focused {
            self.last_focused = "Password";
         }

         if confirm_password_focused {
            self.last_focused = "Confirm Password";
         }

         let target_text = match self.last_focused {
            "Username" => &mut self.username.text,
            "Password" => &mut self.password.text,
            "Confirm Password" => &mut self.confirm_password.text,
            _ => &mut self.username.text,
         };

         ui.add_space(10.0);

         self.virtual_keyboard.show(theme, target_text, ui);
      });
   }
}
