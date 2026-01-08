use eframe::egui::*;
use zeus_theme::{Theme, ThemeKind, window::*};
use zeus_ui_components::CredentialsForm;
use zeus_widgets::Button;

pub struct MyApp {
   pub style_has_been_set: bool,
   pub theme: Theme,
   pub credentials_form: CredentialsForm,
}

impl MyApp {
   fn new(cc: &eframe::CreationContext<'_>) -> Self {
      let theme = Theme::new(ThemeKind::Dark);
      cc.egui_ctx.set_style(theme.style.clone());

      Self {
         style_has_been_set: false,
         theme,
         credentials_form: CredentialsForm::new(),
      }
   }
}

impl eframe::App for MyApp {
   fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
      egui::Rgba::TRANSPARENT.to_array()
   }

   fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
      let theme = self.theme.clone();

      let window = WindowCtx::new("Credentials Form Demo", 40.0, &theme);

      window_frame(ctx, window, |ui| {
         if !self.style_has_been_set {
            ui.ctx().set_style(theme.style.clone());
            self.style_has_been_set = true;
         }

         let bg = self.theme.colors.bg;
         let frame = Frame::new().fill(bg);

         egui::CentralPanel::default()
            .frame(frame)
            .show_inside(ui, |ui| {
               ui.add_space(30.0);

               let visuals = theme.button_visuals();

               ui.vertical_centered(|ui| {
                  ui.set_width(550.0);
                  ui.set_height(350.0);
                  ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                  let open = Button::new("Open Credentials Form").visuals(visuals);
                  if ui.add(open).clicked() {
                     self.credentials_form.open();
                  }

                  let close = Button::new("Close Credentials Form").visuals(visuals);

                  if ui.add(close).clicked() {
                     self.credentials_form.close();
                  }

                  let toggle_confirm_password = Button::new("Toggle Confirm Password").visuals(visuals);

                  if ui.add(toggle_confirm_password).clicked() {
                     let is_confirm_enabled = self.credentials_form.is_confirm_password();
                     self
                        .credentials_form
                        .set_confirm_password(!is_confirm_enabled);
                  }

                  let enable_virtual_keyboard = Button::new("Enable Virtual Keyboard").visuals(visuals);

                  if ui.add(enable_virtual_keyboard).clicked() {
                     self.credentials_form.enable_virtual_keyboard();
                  }

                  let disable_virtual_keyboard = Button::new("Disable Virtual Keyboard").visuals(visuals);

                  if ui.add(disable_virtual_keyboard).clicked() {
                     self.credentials_form.disable_virtual_keyboard();
                  }

                  ui.spacing_mut().button_padding = vec2(4.0, 4.0);
                  let size = vec2(ui.available_width() * 0.6, 20.0);
                  
                  self.credentials_form.set_min_size(size);
                  self.credentials_form.show(&self.theme, ui);
               });
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
         egui_extras::install_image_loaders(&cc.egui_ctx);
         let app = MyApp::new(cc);
         Ok(Box::new(app))
      }),
   )
}
