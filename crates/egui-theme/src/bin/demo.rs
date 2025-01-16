#![cfg_attr(not(feature = "demo"), allow(dead_code, unused_imports))]

#[path = "../lib.rs"]
mod lib;

use egui::{
    Button,
    CentralPanel,
    CollapsingHeader,
    Color32,
    ComboBox,
    Frame,
    RichText,
    Slider,
    Stroke,
    TextEdit,
    UiBuilder,
    ViewportCommand,
};

use lib::{ utils, Theme, ThemeKind, editor::ThemeEditor };

struct DemoApp {
    theme: Theme,
    editor: ThemeEditor,
    on_startup: bool,

    // Dummy values for the demo
    text_edit_text: String,
    coffee_type: Vec<String>,
    selected_coffee: String,
    checked: bool,
    slider_value: f32,
}

impl DemoApp {
    fn new(cc: &eframe::CreationContext) -> Self {
        let theme = Theme::new(ThemeKind::Midnight);
        cc.egui_ctx.set_style(theme.style.clone());

        let coffee_type = vec![
            "Espresso".to_string(),
            "Freddo Espresso".to_string(),
            "French Coffee".to_string(),
            "Greek Coffee".to_string()
        ];

        Self {
            theme,
            editor: ThemeEditor::new(),
            on_startup: true,
            text_edit_text: String::new(),
            coffee_type,
            selected_coffee: "Espresso".to_string(),
            checked: true,
            slider_value: 0.0,
        }
    }
}


#[cfg(feature = "demo")]
fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder
            ::default()
            .with_decorations(false) // Hide the OS-specific "chrome" around the window
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1440.0, 900.0])
            .with_transparent(true), // To have rounded corners we need transparency

        ..Default::default()
    };

    eframe::run_native(
        "egui-Theme Demo",
        options,
        Box::new(|cc| {
            let app = DemoApp::new(&cc);

            Ok(Box::new(app))
        })
    )
}


#[cfg(feature = "demo")]
impl eframe::App for DemoApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let frame = Frame::none().fill(self.theme.colors.bg_color.clone());

        window_frame(ctx, "egui-Theme", frame.clone(), |ui| {
            utils::apply_theme_changes(&self.theme, ui);

            // apply the style again cause for some reason the Window ignores it
            if self.on_startup {
                self.on_startup = false;
                ctx.set_style(self.theme.style.clone());
            }

            // keep the editor open
            self.editor.open = true;

            let new_theme = self.editor.show(&mut self.theme, ui);
            if let Some(theme) = new_theme {
                self.theme = theme;
            }

            egui::CentralPanel
                ::default()
                .frame(frame.clone())
                .show_inside(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(50.0);
                        ui.spacing_mut().item_spacing.y = 5.0;

                        let button = Button::new(RichText::new("Click me!").size(15.0));
                        ui.label("Button");
                        if ui.add(button).clicked() {
                            println!("Button clicked!");
                        }
                        ui.add_space(20.0);

                        ui.label("HyperLink");
                        ui.hyperlink("https://www.egui.rs/");
                        ui.add_space(20.0);

                        let text_edit = TextEdit::singleline(&mut self.text_edit_text).hint_text(
                            "Type something here..."
                        );
                        ui.label("TextEdit");

                        // Override the visuals to give a border only to the TextEdit
                        ui.scope(|ui| {
                            let color1 = self.theme.colors.border_color_idle;
                            let color2 = self.theme.colors.border_color_hover;
                            utils::border_on_idle(ui, 1.0, color1);
                            utils::border_on_hover(ui, 1.0, color2);
                            ui.add(text_edit);
                        });
                        ui.add_space(20.0);

                        ui.horizontal(|ui| {
                            ui.add_space(600.0);
                            ComboBox::from_label("ComboBox")
                                .selected_text(self.selected_coffee.clone())
                                .show_ui(ui, |ui| {
                                    for coffee in &self.coffee_type {
                                        if ui.selectable_label(&self.selected_coffee == coffee, coffee).clicked() {
                                            self.selected_coffee = coffee.clone();
                                        }
                                    }
                                });
                        });

                        ui.add_space(20.0);
                        ui.checkbox(&mut self.checked, "Checkbox");

                        ui.add_space(20.0);
                        ui.horizontal(|ui| {
                            ui.add_space(600.0);
                            ui.add(Slider::new(&mut self.slider_value, 0.0..=100.0).text("Slider"));
                        });

                        ui.add_space(20.0);
                        ui.label("Radio Button");

                        ui.radio_value(&mut self.checked, true, "Yes");
                        ui.radio_value(&mut self.checked, false, "No");

                        ui.add_space(20.0);
                        ui.horizontal(|ui| {
                            ui.add_space(650.0);
                            CollapsingHeader::new("Collapsing Header").show(ui, |ui| {
                                ui.label("This is a collapsible header");
                            });
                        });
                        ui.add_space(20.0);

                        let mut frame1 = self.theme.frame1;
                        let visuals = self.theme.frame1_visuals.clone();
                        utils::frame_it(&mut frame1, Some(visuals), ui, |ui| {
                            ui.set_width(250.0);
                            ui.set_height(50.0);
                            ui.label("Frame 1");
                        });
                        ui.add_space(20.0);

                        let mut frame2 = self.theme.frame2;
                        let visuals = self.theme.frame2_visuals.clone();
                        utils::frame_it(&mut frame2, Some(visuals), ui, |ui| {
                            ui.set_width(250.0);
                            ui.set_height(50.0);
                            ui.label("Frame 2");
                        });
                    });
                });
        });
    }
}

pub fn window_frame(ctx: &egui::Context, title: &str, frame: Frame, add_contents: impl FnOnce(&mut egui::Ui)) {
    CentralPanel::default()
        .frame(frame)
        .show(ctx, |ui| {
            ui.visuals_mut().widgets.noninteractive.bg_stroke = Stroke::NONE;

            let app_rect = ui.max_rect();

            let title_bar_height = 32.0;
            let title_bar_rect = {
                let mut rect = app_rect;
                rect.max.y = rect.min.y + title_bar_height;
                rect
            };
            title_bar_ui(ui, title_bar_rect, title);

            // Add the contents:
            let content_rect = (
                {
                    let mut rect = app_rect;
                    rect.min.y = title_bar_rect.max.y;
                    rect
                }
            ).shrink(4.0);

            let ui_builder = UiBuilder::default().max_rect(content_rect).style(ctx.style().clone());
            let mut content_ui = ui.new_child(ui_builder);
            add_contents(&mut content_ui);
        });
}

fn title_bar_ui(ui: &mut egui::Ui, title_bar_rect: eframe::epaint::Rect, title: &str) {
    use egui::*;

    let painter = ui.painter();

    let title_bar_response = ui.interact(title_bar_rect, Id::new("title_bar"), Sense::click_and_drag());

    // Paint the title:
    painter.text(title_bar_rect.center(), Align2::CENTER_CENTER, title, FontId::proportional(20.0), Color32::WHITE);

    // Paint the line under the title:
    painter.line_segment(
        [title_bar_rect.left_bottom() + vec2(1.0, 0.0), title_bar_rect.right_bottom() + vec2(-1.0, 0.0)],
        ui.visuals().widgets.noninteractive.bg_stroke
    );

    // Interact with the title bar (drag to move window):
    if title_bar_response.double_clicked() {
        let is_maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!is_maximized));
    }

    if title_bar_response.drag_started_by(PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }

    let ui_builder = UiBuilder::default().max_rect(title_bar_rect).style(ui.ctx().style().clone());
    ui.allocate_new_ui(ui_builder, |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.visuals_mut().button_frame = false;
            ui.add_space(8.0);
            close_maximize_minimize(ui);
        });
    });
}

/// Show some close/maximize/minimize buttons for the native window.
fn close_maximize_minimize(ui: &mut egui::Ui) {
    use egui::{ Button, RichText };

    let button_height = 18.0;

    let close_response = ui
        .add(Button::new(RichText::new("❌").size(button_height).color(Color32::WHITE)))
        .on_hover_text("Close the window");
    if close_response.clicked() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
    }

    let is_maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
    if is_maximized {
        let maximized_response = ui
            .add(Button::new(RichText::new("🗗").size(button_height).color(Color32::WHITE)))
            .on_hover_text("Restore window");
        if maximized_response.clicked() {
            ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(false));
        }
    } else {
        let maximized_response = ui
            .add(Button::new(RichText::new("🗗").size(button_height).color(Color32::WHITE)))
            .on_hover_text("Maximize window");
        if maximized_response.clicked() {
            ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(true));
        }
    }

    let minimized_response = ui
        .add(Button::new(RichText::new("🗕").size(button_height).color(Color32::WHITE)))
        .on_hover_text("Minimize the window");
    if minimized_response.clicked() {
        ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
    }
}
