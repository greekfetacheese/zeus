use egui::{ Color32, Stroke, ComboBox, Frame, Ui, Response, Sense };
use super::{ Theme, ThemeKind, FrameVisuals };

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
pub fn apply_theme_changes(theme: &Theme, ui: &mut Ui) {
    ui.style_mut().visuals = theme.style.visuals.clone();

    // text color
    ui.style_mut().visuals.override_text_color = Some(theme.colors.text_color);

    // widget bg color on idle
    ui.style_mut().visuals.widgets.inactive.weak_bg_fill = theme.colors.widget_bg_color_idle;

    // widget bg color on click
    ui.style_mut().visuals.widgets.active.weak_bg_fill = theme.colors.widget_bg_color_click;

    // widget bg color on hover
    ui.style_mut().visuals.widgets.hovered.weak_bg_fill = theme.colors.widget_bg_color_hover;

    // widget bg color on open
    ui.style_mut().visuals.widgets.open.weak_bg_fill = theme.colors.widget_bg_color_open;

    // border color on idle
    ui.style_mut().visuals.widgets.inactive.bg_stroke.color = theme.colors.border_color_idle;

    // border color on click
    ui.style_mut().visuals.widgets.active.bg_stroke.color = theme.colors.border_color_click;

    // border color on hover
    ui.style_mut().visuals.widgets.hovered.bg_stroke.color = theme.colors.border_color_hover;

    // border color on open
    ui.style_mut().visuals.widgets.open.bg_stroke.color = theme.colors.border_color_open;
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
/// When the widget is active (click)
pub fn bg_color_on_click(ui: &mut Ui, color: Color32) {
    ui.visuals_mut().widgets.active.weak_bg_fill = color;
}

/// Give a background color to widgets like Button, ComboxBox, TextEdit, Slider, RadioButton
///
/// When the widget is hovered
pub fn bg_color_on_hover(ui: &mut Ui, color: Color32) {
    ui.visuals_mut().widgets.hovered.weak_bg_fill = color;
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
    add_contents: impl FnOnce(&mut Ui)
) -> Response {
    let mut frame = frame.begin(ui);
    let res = frame.content_ui.scope(|ui| { add_contents(ui) });

    if let Some(visuals) = visuals {
        if res.response.interact(Sense::click()).clicked() {
            frame.frame = frame.frame.fill(visuals.bg_on_click);
            frame.frame = frame.frame.stroke(Stroke::new(visuals.border_on_click.0, visuals.border_on_click.1));
        } else if res.response.hovered() {
            frame.frame = frame.frame.fill(visuals.bg_on_hover);
            frame.frame = frame.frame.stroke(Stroke::new(visuals.border_on_hover.0, visuals.border_on_hover.1));
        }
    }
    frame.end(ui);
    res.response
}
