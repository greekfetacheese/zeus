#![allow(dead_code)]
use egui::*;
use zeus_theme::{Theme, utils::TINT_1};
use zeus_widgets::{
   Button, SecureString,
   secure_text_edit::{SecureTextEdit, SecureTextEditOutput},
};

#[cfg(all(feature = "qr-scanner", target_os = "linux"))]
use super::QRScanner;

const VISIBLE_BLACK: ImageSource<'_> = include_image!("../assets/visible-black.png");
const VISIBLE_WHITE: ImageSource<'_> = include_image!("../assets/visible-white.png");
const INVISIBLE_BLACK: ImageSource<'_> = include_image!("../assets/invisible-black.png");
const INVISIBLE_WHITE: ImageSource<'_> = include_image!("../assets/invisible-white.png");
const QR_CODE_BLACK: ImageSource<'_> = include_image!("../assets/qr-code-black.png");
const QR_CODE_WHITE: ImageSource<'_> = include_image!("../assets/qr-code-white.png");

/// A secure input field that can be used to edit a text containing sensitive information.
///
/// For an example usage see [super::CredentialsForm]
#[derive(Clone)]
pub struct SecureInputField {
   open: bool,
   pub(crate) text: SecureString,
   text_hidden: bool,
   id: &'static str,
   icon_size: Vec2,
   min_size: Vec2,
   #[cfg(all(feature = "qr-scanner", target_os = "linux"))]
   qr_scanner: QRScanner,
   qr_enabled: bool,
}

impl SecureInputField {
   /// Create a new secure input field.
   ///
   /// # Arguments
   ///
   /// * `id` - The id of the input field, this will also be used as the name of the input field
   /// * `text_hidden` - Whether the text is masked
   /// * `open` - Whether the input field is open
   ///
   /// # Panics
   ///
   /// If the `SecureString` allocation fails.
   pub fn new(id: &'static str, text_hidden: bool, open: bool) -> Self {
      Self {
         open,
         text: SecureString::new_with_capacity(32).unwrap(),
         text_hidden,
         id,
         icon_size: vec2(20.0, 20.0),
         min_size: vec2(300.0, 20.0),
         #[cfg(all(feature = "qr-scanner", target_os = "linux"))]
         qr_scanner: QRScanner::new(),
         qr_enabled: true,
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

   /// Erase the text from memory
   pub fn erase(&mut self) {
      self.text.erase();
   }

   pub fn is_text_hidden(&self) -> bool {
      self.text_hidden
   }

   /// Return a clone of the text
   pub fn text(&self) -> SecureString {
      self.text.clone()
   }

   /// Set the id of this input field
   pub fn set_id(&mut self, id: &'static str) {
      self.id = id;
   }

   /// Set whether this input field is hidden
   pub fn set_text_hidden(&mut self, text_hidden: bool) {
      self.text_hidden = text_hidden;
   }

   /// Set the text of this input field
   pub fn set_text(&mut self, text: SecureString) {
      self.text = text;
   }

   /// Set the minimum size of this input field
   pub fn set_min_size(&mut self, size: Vec2) {
      self.min_size = size;
   }

   /// Set the icon size of this input field
   pub fn set_icon_size(&mut self, size: Vec2) {
      self.icon_size = size;
   }

   /// Enable the QR scanner
   pub fn enable_qr_scanner(&mut self) {
      self.qr_enabled = true;
   }

   /// Disable the QR scanner
   pub fn disable_qr_scanner(&mut self) {
      self.qr_enabled = false;
   }

   pub fn with_icon_size(mut self, size: Vec2) -> Self {
      self.icon_size = size;
      self
   }

   pub fn with_min_size(mut self, size: Vec2) -> Self {
      self.min_size = size;
      self
   }

   pub fn with_qr_enabled(mut self, enabled: bool) -> Self {
      self.qr_enabled = enabled;
      self
   }

   /// Show this input field
   ///
   /// # Returns
   /// `SecureTextEditOutput`
   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) -> Option<SecureTextEditOutput> {
      if !self.open {
         return None;
      }

      let ui_size = self.min_size;
      let text_edit_visuals = theme.text_edit_visuals();
      let button_visuals = theme.button_visuals();

      let mut hidden = self.is_text_hidden();
      let field_name = self.id.to_string();
      let img_size = self.icon_size;

      ui.label(RichText::new(field_name).size(theme.text_sizes.large));

      let response = self.text.unlock_mut(|text_str| {
         let text_edit = SecureTextEdit::singleline(text_str)
            .visuals(text_edit_visuals)
            .min_size(ui_size)
            .margin(Margin::same(10))
            .password(hidden)
            .font(FontId::proportional(theme.text_sizes.normal));

         let response = ui.allocate_ui(ui_size, |ui| {
            let res = ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               let output = text_edit.show(ui);

               let img_source = if hidden {
                  match theme.dark_mode {
                     true => INVISIBLE_WHITE,
                     false => INVISIBLE_BLACK,
                  }
               } else {
                  match theme.dark_mode {
                     true => VISIBLE_WHITE,
                     false => VISIBLE_BLACK,
                  }
               };

               let img = if theme.image_tint_recommended {
                  Image::new(img_source)
                     .tint(TINT_1)
                     .fit_to_exact_size(img_size)
               } else {
                  Image::new(img_source).fit_to_exact_size(img_size)
               };

               let button = Button::image(img).visuals(button_visuals);
               if ui.add(button).clicked() {
                  hidden = !hidden;
               }

               #[cfg(all(feature = "qr-scanner", target_os = "linux"))]
               {
                  if self.qr_enabled {
                     let img_source = match theme.dark_mode {
                        true => QR_CODE_WHITE,
                        false => QR_CODE_BLACK,
                     };

                     let img = if theme.image_tint_recommended {
                        Image::new(img_source)
                           .tint(TINT_1)
                           .fit_to_exact_size(img_size)
                     } else {
                        Image::new(img_source).fit_to_exact_size(img_size)
                     };

                     let button = Button::image(img).visuals(button_visuals);
                     if ui.add(button).clicked() {
                        self.qr_scanner.open(ui.ctx().clone());
                     }
                  }
               }
               output
            });
            res
         });
         response
      });

      #[cfg(all(feature = "qr-scanner", target_os = "linux"))]
      {
         if self.qr_enabled {
            self.qr_scanner.show(ui.ctx());
            let res = self.qr_scanner.get_result();
            if let Some(res) = res {
               self.qr_scanner.reset();
               self.set_text(res);
            }
         }
      }

      self.set_text_hidden(hidden);

      Some(response.inner.inner)
   }
}
