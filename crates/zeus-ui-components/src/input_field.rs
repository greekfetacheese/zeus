use egui::*;
use zeus_theme::{Theme, utils::TINT_1};
use zeus_widgets::{Button, SecureString, SecureTextEdit};

#[cfg(feature = "qr-scanner")]
use super::QRScanner;

const VISIBLE_BLACK: ImageSource<'_> = include_image!("../assets/visible-black.png");
const VISIBLE_WHITE: ImageSource<'_> = include_image!("../assets/visible-white.png");
const INVISIBLE_BLACK: ImageSource<'_> = include_image!("../assets/invisible-black.png");
const INVISIBLE_WHITE: ImageSource<'_> = include_image!("../assets/invisible-white.png");
const QR_CODE_BLACK: ImageSource<'_> = include_image!("../assets/qr-code-black.png");
const QR_CODE_WHITE: ImageSource<'_> = include_image!("../assets/qr-code-white.png");

/// A input field that can be used to edit a text.
///
/// For an example usage see [super::CredentialsForm]
pub struct InputField {
   open: bool,
   pub(crate) text: SecureString,
   hidden: bool,
   id: &'static str,
   icon_size: Vec2,
   min_size: Vec2,
   #[cfg(feature = "qr-scanner")]
   qr_scanner: QRScanner,
}

impl InputField {
   /// Create a new input field.
   ///
   /// # Panics
   ///
   /// If the `SecureString` allocation fails.
   pub fn new(id: &'static str, hidden: bool, open: bool) -> Self {
      Self {
         open,
         text: SecureString::new_with_capacity(32).unwrap(),
         hidden,
         id,
         icon_size: vec2(20.0, 20.0),
         min_size: vec2(300.0, 20.0),
         #[cfg(feature = "qr-scanner")]
         qr_scanner: QRScanner::new(),
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

   /// Set a new id for this input field
   pub fn set_id(&mut self, id: &'static str) {
      self.id = id;
   }

   /// Erase the text from memory
   pub fn erase(&mut self) {
      self.text.erase();
   }

   pub fn is_hidden(&self) -> bool {
      self.hidden
   }

   /// Set whether this input field is hidden
   pub fn set_hidden(&mut self, hidden: bool) {
      self.hidden = hidden;
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

   pub fn with_icon_size(mut self, size: Vec2) -> Self {
      self.icon_size = size;
      self
   }

   pub fn with_min_size(mut self, size: Vec2) -> Self {
      self.min_size = size;
      self
   }

   /// Show this input field
   ///
   /// # Returns
   /// `true` if the input field is on focus (selected)
   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) -> bool {
      if !self.open {
         return false;
      }

      let ui_size = self.min_size;
      let text_edit_visuals = theme.text_edit_visuals();
      let button_visuals = theme.button_visuals();

      let mut on_focus = false;
      let mut hidden = self.is_hidden();
      let field_name = self.id.to_string();
      let img_size = self.icon_size;

      ui.label(RichText::new(field_name).size(theme.text_sizes.large));

      self.text.unlock_mut(|text_str| {
         let text_edit = SecureTextEdit::singleline(text_str)
            .visuals(text_edit_visuals)
            .min_size(ui_size)
            .margin(Margin::same(10))
            .password(hidden)
            .font(FontId::proportional(theme.text_sizes.normal));

         ui.allocate_ui(ui_size, |ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               if text_edit.show(ui).response.gained_focus() {
                  on_focus = true;
               }

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

               #[cfg(feature = "qr-scanner")]
               {
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
            });
         });
      });

      #[cfg(feature = "qr-scanner")]
      {
         self.qr_scanner.show(ui.ctx());
         let res = self.qr_scanner.get_result();
         if let Some(res) = res {
            self.qr_scanner.reset();
            self.set_text(res);
         }
      }

      self.set_hidden(hidden);

      on_focus
   }
}
