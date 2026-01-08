use egui::*;
use zeus_theme::Theme;
use zeus_widgets::{Button, SecureString};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputField {
   Username,
   Password,
   ConfirmPassword,
}


/// A virtual keyboard that can be used to edit an input field.
/// 
/// For an example usage see [super::CredentialsForm]
pub struct VirtualKeyboard {
   open: bool,
   active_target: InputField,
   shift_active: bool,
   caps_lock_active: bool,
}

impl VirtualKeyboard {
   pub fn new(open: bool) -> Self {
      Self {
         open,
         active_target: InputField::Username,
         shift_active: false,
         caps_lock_active: false,
      }
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn set_active_target(&mut self, target: InputField) {
      self.active_target = target;
   }

   pub fn show(
      &mut self,
      ui: &mut Ui,
      theme: &Theme,
      target_str: &mut SecureString,
   ) {
      if !self.open {
         return;
      }

      // Define the keyboard layout
      let keys_layout_lower = vec![
         vec![
            "`",
            "1",
            "2",
            "3",
            "4",
            "5",
            "6",
            "7",
            "8",
            "9",
            "0",
            "-",
            "=",
            "Backspace",
         ],
         vec![
            "q", "w", "e", "r", "t", "y", "u", "i", "o", "p", "[", "]", "\\",
         ],
         vec![
            "Caps", "a", "s", "d", "f", "g", "h", "j", "k", "l", ";", "'", "Enter",
         ],
         vec![
            "Shift", "z", "x", "c", "v", "b", "n", "m", ",", ".", "/", "Shift",
         ],
      ];
      let keys_layout_upper = vec![
         vec![
            "~",
            "!",
            "@",
            "#",
            "$",
            "%",
            "^",
            "&",
            "*",
            "(",
            ")",
            "_",
            "+",
            "Backspace",
         ],
         vec![
            "Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P", "{", "}", "|",
         ],
         vec![
            "Caps", "A", "S", "D", "F", "G", "H", "J", "K", "L", ":", "\"", "Enter",
         ],
         vec![
            "Shift", "Z", "X", "C", "V", "B", "N", "M", "<", ">", "?", "Shift",
         ],
      ];

      let frame = theme.frame2;
      let button_visuals = theme.button_visuals();

      frame.show(ui, |ui| {
         ui.vertical(|ui| {
            let is_uppercase = self.shift_active ^ self.caps_lock_active;
            let layout = if is_uppercase {
               &keys_layout_upper
            } else {
               &keys_layout_lower
            };

            for row in layout {
               ui.horizontal(|ui| {
                  for &key in row {
                     let text = RichText::new(key).size(theme.text_sizes.normal);
                     let key_button = Button::new(text)
                        .visuals(button_visuals)
                        .min_size(vec2(30.0, 30.0));
                     if ui.add(key_button).clicked() {
                        self.handle_key_press(key, target_str);
                     }
                  }
               });
            }
            // Spacebar
            ui.horizontal(|ui| {
               let button = Button::new(" ")
                  .visuals(button_visuals)
                  .min_size(vec2(30.0 * 5.0, 30.0));
               if ui.add(button).clicked() {
                  target_str.push_str(" ");
               }
            });
         });
      });
   }

   fn handle_key_press(&mut self, key: &str, target: &mut SecureString) {
      match key {
         "Backspace" => {
            target.unlock_mut(|s| {
               let len = s.char_len();
               if len > 0 {
                  s.delete_text_char_range(len - 1..len);
               }
            });
         }
         "Shift" => {
            self.shift_active = !self.shift_active;
         }
         "Caps" => {
            self.caps_lock_active = !self.caps_lock_active;
            self.shift_active = false; // Typically, pressing Caps disables Shift
         }
         "Enter" => {
            // For now, we do nothing.
         }
         _ => {
            target.push_str(key);
            // Deactivate shift after a character press
            if self.shift_active {
               self.shift_active = false;
            }
         }
      }
   }
}
