use eframe::egui::*;
use egui_theme::{Theme, ThemeEditor, ThemeKind, utils, window::window_frame};

struct DemoApp {
   set_theme: bool,
   theme: Theme,
   editor: ThemeEditor,
}

impl DemoApp {
   fn new(cc: &eframe::CreationContext<'_>) -> Self {
      let theme = Theme::new(ThemeKind::Nord);
      let editor = ThemeEditor::new();
      cc.egui_ctx.set_style(theme.style.clone());

      Self {
         set_theme: false,
         theme,
         editor,
      }
   }
}

impl eframe::App for DemoApp {
   fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
      egui::Rgba::TRANSPARENT.to_array()
   }

   fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
      let theme2 = self.theme.clone();

      window_frame(ctx, "egui Theme Demo", theme2, |ui| {
         utils::apply_theme_changes(&self.theme, ui);

         let bg_color = self.theme.colors.bg_color;
         let frame = Frame::new().fill(bg_color);
         let left_panel_frame = self.theme.frame2;

         egui::SidePanel::left("left_panel")
            .min_width(150.0)
            .max_width(150.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(left_panel_frame)
            .show_inside(ui, |ui| {
               ui.vertical_centered(|ui| {
                  ui.set_max_width(100.0);
                  utils::bg_color_on_idle(ui, Color32::TRANSPARENT);
                  utils::no_border(ui);

                  let text_size = self.theme.text_sizes.large;

                  let home_text = RichText::new("Home").size(text_size);
                  let home_button = Button::new(home_text).min_size(vec2(100.0, 50.0));
                  ui.add(home_button);

                  let settings_text = RichText::new("Settings").size(text_size);
                  let settings_button = Button::new(settings_text).min_size(vec2(100.0, 50.0));
                  ui.add(settings_button);

                  let editor_text = RichText::new("Toggle Editor").size(text_size);
                  let editor_button = Button::new(editor_text).min_size(vec2(100.0, 50.0));
                  if ui.add(editor_button).clicked() {
                     self.editor.open = !self.editor.open;
                  }

                  let about_text = RichText::new("About").size(text_size);
                  let about_button = Button::new(about_text).min_size(vec2(100.0, 50.0));
                  ui.add(about_button);
               });
            });

         egui::CentralPanel::default().frame(frame).show_inside(ui, |ui| {
            if !self.set_theme {
               ctx.set_style(self.theme.style.clone());
               self.set_theme = true;
            }

            let new_theme = self.editor.show(&mut self.theme, ui);
            if let Some(new_theme) = new_theme {
               self.theme = new_theme;
            }
         });
      });
   }
}

fn main() -> eframe::Result {
   let options = eframe::NativeOptions {
      viewport: egui::ViewportBuilder::default()
         .with_decorations(false)
         .with_inner_size([800.0, 800.0])
         .with_transparent(true)
         .with_resizable(true),
      ..Default::default()
   };

   eframe::run_native(
      "egui Theme Demo",
      options,
      Box::new(|cc| {
         let app = DemoApp::new(cc);
         Ok(Box::new(app))
      }),
   )
}
