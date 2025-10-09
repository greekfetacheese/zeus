use super::{Theme, FrameVisuals, ThemeKind};
use egui::{Color32, Frame, Sense, Response, ComboBox, Stroke, Ui};

// Tint/Overlay colors, useful for example if you want to slightly darken an image on the fly

/// Should work for most images that are shown on a very dark background
pub const TINT_1: Color32 = Color32::from_rgba_premultiplied(216, 216, 216, 255);

/// Show a ComboBox to change the theme
///
/// Returns the new theme if we select one, the new theme is also applied to the [egui::Context]
pub fn change_theme(current_theme: &Theme, ui: &mut Ui) -> Option<Theme> {
   let mut new_theme_opt = None;
   ComboBox::from_label("Theme")
      .selected_text(current_theme.kind.to_str())
      .show_ui(ui, |ui| {
         for kind in ThemeKind::to_vec() {
            if ui.selectable_label(current_theme.kind == kind, kind.to_str()).clicked() {
               let new_theme = Theme::new(kind);
               ui.ctx().set_style(new_theme.style.clone());
               new_theme_opt = Some(new_theme);
            }
         }
      });
   new_theme_opt
}

/// Apply any theme changes to the ui
pub fn apply_theme_changes(theme: &mut Theme, ui: &mut Ui) {
   theme.set_window_frame_colors();
   theme.set_frame1_colors();
   theme.set_frame2_colors();
   
   ui.style_mut().visuals = theme.style.visuals.clone();
}

// Helper functions to override the visuals

/// Removes the border from widgets like Button, ComboxBox, TextEdit, Slider, RadioButton
///
/// When the widget is inactive (no hover, clicks etc..)
pub fn no_border_on_idle(ui: &mut Ui) {
   ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::NONE;
}

/// Removes the border from widgets like Button, ComboxBox, TextEdit, Slider, RadioButton
///
/// When the widget is active (click)
pub fn no_border_on_click(ui: &mut Ui) {
   ui.visuals_mut().widgets.active.bg_stroke = Stroke::NONE;
}

/// Removes the border from widgets like Button, ComboxBox, TextEdit, Slider, RadioButton
///
/// When the widget is hovered
pub fn no_border_on_hover(ui: &mut Ui) {
   ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::NONE;
}

/// Removes the border from widgets like Button, ComboxBox, TextEdit, Slider, RadioButton
///
/// At any state
pub fn no_border(ui: &mut Ui) {
   ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::NONE;
   ui.visuals_mut().widgets.active.bg_stroke = Stroke::NONE;
   ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::NONE;
}

/// Give a border to widgets like Button, ComboxBox, TextEdit, Slider, RadioButton
///
/// When the widget is inactive (no hover, clicks etc..)
pub fn border_on_idle(ui: &mut Ui, width: f32, color: Color32) {
   ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(width, color);
}

/// Give a border to widgets like Button, ComboxBox, TextEdit, Slider, RadioButton
///
/// When the widget is active (click)
pub fn border_on_click(ui: &mut Ui, width: f32, color: Color32) {
   ui.visuals_mut().widgets.active.bg_stroke = Stroke::new(width, color);
}

/// Give a border to widgets like ComboxBox
///
/// When the widget is open
pub fn border_on_open(ui: &mut Ui, width: f32, color: Color32) {
   ui.visuals_mut().widgets.open.bg_stroke = Stroke::new(width, color);
}

/// Give a border to widgets like Button, ComboxBox, TextEdit, Slider, RadioButton
///
/// When the widget is hovered
pub fn border_on_hover(ui: &mut Ui, width: f32, color: Color32) {
   ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::new(width, color);
}

/// Give a background color to widgets like Button, ComboxBox, TextEdit, Slider, RadioButton
///
/// When the widget is inactive (no hover, clicks etc..)
pub fn bg_color_on_idle(ui: &mut Ui, color: Color32) {
   ui.visuals_mut().widgets.inactive.weak_bg_fill = color;
}

/// Give a background color to widgets like Button, ComboxBox, TextEdit, Slider, RadioButton
///
/// When the widget is hovered
pub fn bg_color_on_hover(ui: &mut Ui, color: Color32) {
   ui.visuals_mut().widgets.hovered.weak_bg_fill = color;
}

/// Give a background color to widgets like Button, ComboxBox, TextEdit, Slider, RadioButton
///
/// When the widget is active (click)
pub fn bg_color_on_click(ui: &mut Ui, color: Color32) {
   ui.visuals_mut().widgets.active.weak_bg_fill = color;
}

/// Give a background color to widgets like Button, ComboxBox, TextEdit, Slider, RadioButton
pub fn bg_color_on_open(ui: &mut Ui, color: Color32) {
   ui.visuals_mut().widgets.open.weak_bg_fill = color;
}

/// Window Fill Color
///
/// It also affects the bg color of an opened ComboBox
pub fn window_fill(ui: &mut Ui, color: Color32) {
   ui.visuals_mut().window_fill = color;
}

/// Window Border
///
/// It also affects the border of an opened ComboBox
pub fn window_border(ui: &mut Ui, width: f32, color: Color32) {
   ui.visuals_mut().window_stroke = Stroke::new(width, color);
}

/// Put this ui on top of this frame
pub fn frame_it(
   frame: &mut Frame,
   visuals: Option<FrameVisuals>,
   ui: &mut Ui,
   add_contents: impl FnOnce(&mut Ui),
) -> Response {
   let mut frame = frame.begin(ui);
   let res = frame.content_ui.scope(|ui| add_contents(ui));

   if let Some(visuals) = visuals {
      if res.response.interact(Sense::click()).clicked() {
         frame.frame = frame.frame.fill(visuals.bg_on_click);
         frame.frame = frame.frame.stroke(Stroke::new(
            visuals.border_on_click.0,
            visuals.border_on_click.1,
         ));
      } else if res.response.hovered() {
         frame.frame = frame.frame.fill(visuals.bg_on_hover);
         frame.frame = frame.frame.stroke(Stroke::new(
            visuals.border_on_hover.0,
            visuals.border_on_hover.1,
         ));
      }
   }
   frame.end(ui);
   res.response
}
