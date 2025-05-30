use egui;
use egui_widgets::text_edit::SecureTextEdit;
use secure_types::SecureString;

pub struct App {
   pub secret_text: SecureString,
   pub secret_text2: SecureString,
   pub secret_text3: SecureString,
   pub normal_text: String,
   pub hide_texts: bool,
   pub show_msg: bool,
}

impl App {
   pub fn new(_cc: &eframe::CreationContext) -> Self {
      Self {
         secret_text: SecureString::new_with_capacity(1024).unwrap(),
         secret_text2: SecureString::new_with_capacity(1024).unwrap(),
         secret_text3: SecureString::new_with_capacity(1024).unwrap(),
         normal_text: String::new(),
         hide_texts: false,
         show_msg: false,
      }
   }
}

fn main() -> eframe::Result {
   let options = eframe::NativeOptions {
      viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 500.0]),
      ..Default::default()
   };

   eframe::run_native(
      "Secure String Test",
      options,
      Box::new(|cc| {
         let app = App::new(&cc);
         Ok(Box::new(app))
      }),
   )
}

impl eframe::App for App {
   fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
      egui::CentralPanel::default().show(ctx, |ui| {
         ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 10.0;

            if !self.hide_texts {
            ui.label("Secret Text:");
            self.secret_text.mut_scope(|text| {
               let text_edit = SecureTextEdit::singleline(text)
                  .min_size(egui::vec2(200.0, 30.0))
                  .hint_text("This is a hint");
               text_edit.show(ui);
            });

            ui.separator();

            ui.label("Secret Text 2:");
            self.secret_text2.mut_scope(|text| {
               let text_edit = SecureTextEdit::singleline(text)
                  .password(true)
                  .min_size(egui::vec2(200.0, 30.0));
               text_edit.show(ui);
            });
         }

            ui.separator();

            /*
            ui.label("Secret Text 3:");
            self.secret_text3.mut_scope(|text| {
               let text_edit = SecureTextEdit::multiline(text)
                  .password(false)
                  .min_size(egui::vec2(200.0, 30.0));
               text_edit.show(ui);
            });
            */

            // ui.separator();

            // ui.label("Normal Text:");
            // let text_edit = egui::TextEdit::multiline(&mut self.normal_text).min_size(egui::vec2(200.0, 30.0));
            // text_edit.show(ui);

            if ui.button("Erase").clicked() {
               self.secret_text.erase();
               self.secret_text2.erase();
               self.secret_text3.erase();
               self.show_msg = true;
            }

            if ui.button("Println!").clicked() {
               self.secret_text.str_scope(|text| {
                  println!("{}", text);
               });
               self.secret_text2.str_scope(|text| {
                  println!("{}", text);
               });
            }

            if ui.button("Hide TextEdits").clicked() {
               self.hide_texts = !self.hide_texts;
            }
         });

         if self.show_msg {
            egui::Window::new("Message")
               .collapsible(false)
               .resizable(false)
               .show(ctx, |ui| {
                  ui.label("The secret text has been erased");
                  if ui.button("Ok").clicked() {
                     self.show_msg = false;
                  }
               });
         }
      });
   }
}
