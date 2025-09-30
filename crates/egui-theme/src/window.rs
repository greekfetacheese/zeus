use super::{Theme, utils};
use egui::{self, Button, CentralPanel, Color32, Rect, RichText, UiBuilder, ViewportCommand, vec2};

/// A frame for the App's Native window
pub fn window_frame(
   ctx: &egui::Context,
   title: &str,
   theme: Theme,
   add_contents: impl FnOnce(&mut egui::Ui),
) {
   CentralPanel::default().frame(theme.window_frame).show(ctx, |ui| {
      let app_rect = ui.max_rect();

      let title_bar_height = 32.0;
      let title_bar_rect = {
         let mut rect = app_rect;
         rect.max.y = rect.min.y + title_bar_height;
         rect
      };

      title_bar_ui(ui, &theme, title_bar_rect, title);

      // Add the contents:
      let content_rect = {
         let mut rect = app_rect;
         rect.min.y = title_bar_rect.max.y;
         rect
      }
      .shrink(4.0);

      let ui_builder = UiBuilder::default().max_rect(content_rect).style(ctx.style().clone());
      let mut content_ui = ui.new_child(ui_builder);
      add_contents(&mut content_ui);
   });
}

fn title_bar_ui(ui: &mut egui::Ui, theme: &Theme, title_bar_rect: Rect, title: &str) {
   use egui::*;

   let painter = ui.painter();

   let title_bar_response = ui.interact(
      title_bar_rect,
      Id::new("title_bar"),
      Sense::click_and_drag(),
   );

   let title_size = theme.text_sizes.heading;
   let title_color = theme.colors.text_color;

   // Paint the title:
   painter.text(
      title_bar_rect.center(),
      Align2::CENTER_CENTER,
      title,
      FontId::proportional(title_size),
      title_color,
   );

   // Paint the line under the title:
   painter.line_segment(
      [
         title_bar_rect.left_bottom() + vec2(1.0, 0.0),
         title_bar_rect.right_bottom() + vec2(-1.0, 0.0),
      ],
      ui.visuals().widgets.noninteractive.bg_stroke,
   );

   // Interact with the title bar (drag to move window):
   if title_bar_response.double_clicked() {
      let is_maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
      ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!is_maximized));
   }

   if title_bar_response.drag_started_by(PointerButton::Primary) {
      ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
   }

   let ui_builder = UiBuilder::default().max_rect(title_bar_rect).style(theme.style.clone());

   ui.scope_builder(ui_builder, |ui| {
      ui.with_layout(
         egui::Layout::right_to_left(egui::Align::Center),
         |ui| {
            ui.add_space(5.0);
            close_maximize_minimize(ui, &theme);
         },
      );
   });
}

/// Show some close/maximize/minimize buttons for the native window.
fn close_maximize_minimize(ui: &mut egui::Ui, theme: &Theme) {
   utils::bg_color_on_idle(ui, Color32::TRANSPARENT);
   utils::no_border(ui);
   ui.style_mut().visuals.widgets.hovered.expansion = 5.0;

   let button_size = vec2(20.0, 15.0);
   let color = theme.colors.text_color;

   ui.scope(|ui| {
      utils::bg_color_on_hover(ui, theme.colors.error_color);
      let text = RichText::new("❌").color(color).size(theme.text_sizes.large);
      let close_button = Button::new(text).min_size(button_size);
      let close_response = ui.add(close_button);

      if close_response.clicked() {
         ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
      }
   });

   let is_maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));

   if is_maximized {
      let text = RichText::new("🗗").color(color).size(theme.text_sizes.large);
      let maximized_button = Button::new(text).min_size(button_size);
      let maximized_response = ui.add(maximized_button);

      if maximized_response.clicked() {
         ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(false));
      }
   } else {
      let text = RichText::new("🗗").color(color).size(theme.text_sizes.large);
      let maximized_button = Button::new(text).min_size(button_size);
      let maximized_response = ui.add(maximized_button);

      if maximized_response.clicked() {
         ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(true));
      }
   }

   let text = RichText::new("🗕").color(color).size(theme.text_sizes.large);
   let minimized_button = Button::new(text).min_size(button_size);
   let minimized_response = ui.add(minimized_button);

   if minimized_response.clicked() {
      ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
   }
}
