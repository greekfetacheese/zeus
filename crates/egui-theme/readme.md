# egui-Theme

## Theme selection & customization for egui

### Usage:

``` rust
use egui::Context;
use egui_theme::{Theme, ThemeKind, ThemeEditor, utils};

struct MyApp {
    theme: Theme,
    // this is optional
    editor: ThemeEditor
}

impl MyApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // set the midnight theme
        let theme = Theme::new(ThemeKind::Midnight);
        // set the theme's style to egui
        cc.egui_ctx.set_style(theme.style.clone());
        Self { theme, editor: ThemeEditor::new() }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {

            // apply any changes we made to the ui
            utils::apply_theme_changes(&self.theme, ui);

            // keep the editor open
            self.editor.open = true;
            
            // show the editor and get the new theme
            let new_theme = self.editor.show(&mut self.theme, ui);
            if let Some(theme) = new_theme {
                self.theme = theme;
            }

            // utils has shortucts to override the ui visuals
            // example:
            ui.button("Click Me!");

            // Override the visuals to give a border only to the TextEdit
            ui.scope(|ui| {
            let text_edit = TextEdit::singleline(&mut self.text_edit_text).hint_text(
            "Type something here...");

                let color1 = self.theme.colors.border_color_idle;
                let color2 = self.theme.colors.border_color_hover;
                utils::border_on_idle(ui, 1.0, color1);
                utils::border_on_hover(ui, 1.0, color2);
                ui.add(text_edit);
            });

        });
    }
}
```

### Theme can also be serialized to be saved as a custom theme

``` rust
let theme_data = self.theme.to_json().unwrap();
let path = std::path::PathBuf::from("my-custom-theme.json");
std::fs::write(&save_path, data).unwrap();
```

### Load a custom theme

``` rust
let path = std::path::PathBuf::from("my-custom-theme.json");
let custom_theme = Theme::from_custom(path).unwrap();
```

#### You can also run the demo app to preview and customize themes

``` rust
cargo run --features demo --bin demo
```