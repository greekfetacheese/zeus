//! Custom viewport title bar (minimize / maximize / close + drag).

use egui::{
   Align, Align2, Button, CentralPanel, Color32, CornerRadius, FontId, Id, Layout, PointerButton,
   Rect, RichText, Sense, Shadow, Stroke, StrokeKind, Ui, UiBuilder, ViewportCommand, pos2, vec2,
};
use egui_elements::{Elevation, Theme};

pub struct WindowCtx {
   pub title: String,
   pub frame: egui::Frame,
   /// Transparent margin between the window edge and the card.
   pub pad: f32,
   /// Fill behind the title bar and the page.
   pub card_fill: Color32,
   /// 1 px outline traced around the card.
   pub card_stroke: Stroke,
   /// Corner radius shared by the card, its outline and the bar's top corners.
   pub card_radius: CornerRadius,
   /// Painted into the margin so the card floats above whatever is behind it.
   pub card_shadow: Shadow,
   pub bar_height: f32,
   pub title_bar_fill: Color32,
   pub title_text_size: f32,
   pub title_text_color: Color32,
   pub line_stroke: Stroke,
   pub button_text_size: f32,
   pub button_text_color: Color32,
   pub close_on_hover_color: Color32,
   pub on_hover_color: Color32,
}

impl WindowCtx {
   pub fn settings(theme: &Theme) -> Self {
      Self {
         title: "Settings".to_string(),
         frame: egui::Frame::NONE,
         pad: 10.0,
         card_fill: theme.colors.bg,
         card_stroke: Stroke::new(1.0, theme.colors.border),
         card_radius: CornerRadius::same(theme.corner_radius),
         card_shadow: theme.shadow(Elevation::Overlay),
         bar_height: 36.0,
         title_bar_fill: theme.colors.title_bar,
         title_text_size: theme.typography.normal,
         title_text_color: theme.colors.text,
         line_stroke: Stroke::new(1.0, theme.colors.border),
         button_text_size: 14.0,
         button_text_color: theme.colors.text,
         close_on_hover_color: theme.colors.error,
         on_hover_color: theme.colors.hover,
      }
   }
}

pub fn window_frame(ui: &mut Ui, window_ctx: WindowCtx, add_contents: impl FnOnce(&mut Ui)) {
   CentralPanel::default().frame(window_ctx.frame).show(ui, |ui| {
      let card = ui.max_rect().shrink(window_ctx.pad);

      let title_bar_rect = {
         let mut rect = card;
         rect.max.y = card.min.y + window_ctx.bar_height;
         rect
      };

      let painter = ui.painter();

      painter.add(window_ctx.card_shadow.as_shape(card, window_ctx.card_radius));
      painter.rect_filled(card, window_ctx.card_radius, window_ctx.card_fill);
      painter.rect_filled(
         title_bar_rect,
         CornerRadius {
            nw: window_ctx.card_radius.nw,
            ne: window_ctx.card_radius.ne,
            sw: 0,
            se: 0,
         },
         window_ctx.title_bar_fill,
      );
      // Outline last, so the bar's fill cannot paint over the card's border.
      painter.rect(
         card,
         window_ctx.card_radius,
         Color32::TRANSPARENT,
         window_ctx.card_stroke,
         StrokeKind::Inside,
      );

      title_bar_ui(ui, &window_ctx, title_bar_rect);

      let content_rect = {
         let mut rect = card;
         rect.min.y += window_ctx.bar_height;
         // Stop above the bottom arc, or the opaque page covers the card's corners.
         rect.max.y -= f32::from(window_ctx.card_radius.sw);
         rect
      };

      let ui_builder = UiBuilder::default().max_rect(content_rect).style(ui.style().clone());
      let mut content_ui = ui.new_child(ui_builder);
      content_ui.set_clip_rect(content_rect);
      add_contents(&mut content_ui);
   });
}

fn title_bar_ui(ui: &mut Ui, window: &WindowCtx, title_bar_rect: Rect) {
   let painter = ui.painter();

   let title_bar_response = ui.interact(
      title_bar_rect,
      Id::unique("settings_title_bar"),
      Sense::click_and_drag(),
   );

   painter.text(
      title_bar_rect.center(),
      Align2::CENTER_CENTER,
      window.title.clone(),
      FontId::proportional(window.title_text_size),
      window.title_text_color,
   );

   let y = title_bar_rect.bottom() - window.line_stroke.width / 2.0;
   painter.line_segment(
      [
         pos2(title_bar_rect.left() + 1.0, y),
         pos2(title_bar_rect.right() - 1.0, y),
      ],
      window.line_stroke,
   );

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
         close_maximize_minimize(ui, window);
      });
   });
}

fn close_maximize_minimize(ui: &mut Ui, window: &WindowCtx) {
   ui.spacing_mut().button_padding = vec2(0.0, 0.0);
   ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

   ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
   ui.visuals_mut().widgets.inactive.bg_fill = Color32::TRANSPARENT;
   ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::NONE;
   ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::NONE;
   ui.visuals_mut().widgets.active.bg_stroke = Stroke::NONE;

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
         ui.visuals_mut().widgets.hovered.weak_bg_fill = hover_color;
         ui.visuals_mut().widgets.hovered.bg_fill = hover_color;
         ui.visuals_mut().widgets.active.weak_bg_fill = hover_color;
         ui.visuals_mut().widgets.active.bg_fill = hover_color;
         let rich_text = RichText::new(text).color(text_color).size(text_size);
         let button = Button::new(rich_text).min_size(button_size);

         ui.add_sized(button_size, button).clicked()
      })
      .inner
   };

   if add_title_button(ui, "❌", window.close_on_hover_color) {
      ui.ctx().send_viewport_cmd(ViewportCommand::Close);
   }

   let is_maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));

   let max_icon = if is_maximized { "🗗" } else { "🗖" };
   if add_title_button(ui, max_icon, window.on_hover_color) {
      ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!is_maximized));
   }

   if add_title_button(ui, "🗕", window.on_hover_color) {
      ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
   }
}
