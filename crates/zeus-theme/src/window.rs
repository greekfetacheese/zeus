use super::{Theme, utils};
use egui::*;

pub struct WindowCtx {
   pub frame: Frame,
   pub title: String,
   pub bar_height: f32,
   pub title_text_size: f32,
   pub title_text_color: Color32,
   pub line_stroke: Stroke,
   pub button_text_size: f32,
   pub button_text_color: Color32,
   pub on_hover_color: Color32,
   pub close_on_hover_color: Color32,
}

impl WindowCtx {
   pub fn new(title: &str, bar_height: f32, theme: &Theme) -> Self {
      let frame = theme.window_frame;
      let title_text_size = theme.text_sizes.heading;
      let title_text_color = theme.colors.text;
      let line_stroke = Stroke::new(0.0, theme.colors.border);
      let button_text_size = theme.text_sizes.large;
      let button_text_color = theme.colors.text;
      let on_hover_color = theme.colors.hover;
      let close_on_hover_color = theme.colors.error;

      Self {
         frame,
         title: title.to_owned(),
         bar_height,
         title_text_size,
         title_text_color,
         line_stroke,
         button_text_size,
         button_text_color,
         on_hover_color,
         close_on_hover_color,
      }
   }

   pub fn with_frame(mut self, frame: Frame) -> Self {
      self.frame = frame;
      self
   }

   pub fn with_line_stroke(mut self, stroke: Stroke) -> Self {
      self.line_stroke = stroke;
      self
   }

   pub fn with_title_text_size(mut self, size: f32) -> Self {
      self.title_text_size = size;
      self
   }

   pub fn with_title_text_color(mut self, color: Color32) -> Self {
      self.title_text_color = color;
      self
   }

   pub fn with_button_text_color(mut self, color: Color32) -> Self {
      self.button_text_color = color;
      self
   }

   pub fn with_on_hover_color(mut self, color: Color32) -> Self {
      self.on_hover_color = color;
      self
   }

   pub fn with_close_on_hover_color(mut self, color: Color32) -> Self {
      self.close_on_hover_color = color;
      self
   }
}

pub fn window_frame(ctx: &Context, window_ctx: WindowCtx, add_contents: impl FnOnce(&mut Ui)) {
   CentralPanel::default().frame(window_ctx.frame).show(ctx, |ui| {
      let app_rect = ui.max_rect();

      let title_bar_rect = {
         let mut rect = app_rect;
         rect.max.y = window_ctx.bar_height;
         rect
      };

      title_bar_ui(ui, &window_ctx, title_bar_rect);

      // Add the contents
      let content_rect = {
         let mut rect = app_rect;
         rect.min.y = window_ctx.bar_height;
         rect
      };

      let ui_builder = UiBuilder::default().max_rect(content_rect).style(ctx.style().clone());
      let mut content_ui = ui.new_child(ui_builder);
      add_contents(&mut content_ui);
   });
}

fn title_bar_ui(ui: &mut Ui, window: &WindowCtx, title_bar_rect: Rect) {
   let painter = ui.painter();

   let title_bar_response = ui.interact(
      title_bar_rect,
      Id::new("title_bar"),
      Sense::click_and_drag(),
   );

   // Paint the title:
   painter.text(
      title_bar_rect.center(),
      Align2::CENTER_CENTER,
      window.title.clone(),
      FontId::proportional(window.title_text_size),
      window.title_text_color,
   );

   // Paint the line under the title:
   let y = title_bar_rect.bottom() - window.line_stroke.width / 2.0;
   painter.line_segment(
      [
         pos2(title_bar_rect.left() + 1.0, y),
         pos2(title_bar_rect.right() - 1.0, y),
      ],
      window.line_stroke,
   );

   // Interact with the title bar (drag to move window):
   if title_bar_response.double_clicked() {
      let is_maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
      ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!is_maximized));
   }

   if title_bar_response.drag_started_by(PointerButton::Primary) {
      ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
   }

   let ui_builder = UiBuilder::default().max_rect(title_bar_rect).style(ui.style().clone());
   let layout = Layout::right_to_left(Align::Center);

   ui.scope_builder(ui_builder, |ui| {
      ui.with_layout(layout, |ui| {
         close_maximize_minimize(ui, &window);
      });
   });
}

/// Show some close/maximize/minimize buttons for the native window.
fn close_maximize_minimize(ui: &mut Ui, window: &WindowCtx) {
   ui.spacing_mut().button_padding = vec2(0.0, 0.0);
   ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

   utils::bg_color_on_idle(ui, Color32::TRANSPARENT);
   utils::no_border(ui);

   ui.style_mut().visuals.widgets.inactive.expansion = 0.0;
   ui.style_mut().visuals.widgets.hovered.expansion = 0.0;
   ui.style_mut().visuals.widgets.active.expansion = 0.0;
   ui.style_mut().visuals.widgets.inactive.corner_radius = CornerRadius::ZERO;
   ui.style_mut().visuals.widgets.hovered.corner_radius = CornerRadius::ZERO;
   ui.style_mut().visuals.widgets.active.corner_radius = CornerRadius::ZERO;

   let button_size = vec2(45.0, window.bar_height);
   let text_size = window.button_text_size;
   let text_color = window.button_text_color;

   let add_title_button = |ui: &mut Ui, text: &str, hover_color: Color32| -> bool {
      ui.scope(|ui| {
         utils::bg_color_on_hover(ui, hover_color);
         let rich_text = RichText::new(text).color(text_color).size(text_size);
         let button = Button::new(rich_text).min_size(button_size);

         ui.add_sized(button_size, button).clicked()
      })
      .inner
   };

   // Close Button
   if add_title_button(ui, "❌", window.close_on_hover_color) {
      ui.ctx().send_viewport_cmd(ViewportCommand::Close);
   }

   let is_maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));

   // Maximize/Restore
   let max_icon = if is_maximized { "🗗" } else { "🗖" };
   if add_title_button(ui, max_icon, window.on_hover_color) {
      ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!is_maximized));
   }

   // Minimize
   if add_title_button(ui, "🗕", window.on_hover_color) {
      ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
   }
}
