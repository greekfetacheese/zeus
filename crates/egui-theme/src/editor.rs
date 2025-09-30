use egui::{
   CollapsingHeader, ComboBox, Frame, ScrollArea, Slider, Ui, Window, vec2,
   widgets::color_picker::{Alpha, color_edit_button_srgba},
};

use super::{FrameVisuals, Theme, utils};

/// Identify which state of the widget we should edit
#[derive(Clone, PartialEq)]
pub enum WidgetState {
   NonInteractive,
   Inactive,
   Hovered,
   Active,
   Open,
}

impl WidgetState {
   /// Convert the state to a string
   pub fn to_str(&self) -> &'static str {
      match self {
         WidgetState::NonInteractive => "Non-interactive",
         WidgetState::Inactive => "Inactive",
         WidgetState::Hovered => "Hovered",
         WidgetState::Active => "Active",
         WidgetState::Open => "Open",
      }
   }

   /// Convert the state to a vector
   pub fn to_vec(&self) -> Vec<WidgetState> {
      let non_interactive = Self::NonInteractive;
      let inactive = Self::Inactive;
      let hovered = Self::Hovered;
      let active = Self::Active;
      let open = Self::Open;

      vec![non_interactive, inactive, hovered, active, open]
   }
}

#[derive(Clone)]
pub struct ThemeEditor {
   pub open: bool,
   /// The current widget state being edited
   pub widget_state: WidgetState,
   pub size: (f32, f32),
}

impl ThemeEditor {
   pub fn new() -> Self {
      Self {
         open: false,
         widget_state: WidgetState::NonInteractive,
         size: (300.0, 300.0),
      }
   }

   /// Show the theme editor in a window
   ///
   /// Returns the new theme if we change it
   pub fn show(&mut self, theme: &mut Theme, ui: &mut Ui) -> Option<Theme> {
      if !self.open {
         return None;
      }

      let mut open = self.open;
      let mut new_theme = None;
      Window::new("Theme Editor")
         .open(&mut open)
         .resizable([true, true])
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_min_width(self.size.0);
            ui.set_min_height(self.size.1);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            new_theme = utils::change_theme(theme, ui);

            ui.add_space(20.0);

            ScrollArea::vertical().show(ui, |ui| {
               ui.set_width(self.size.0);
               ui.set_height(self.size.1);
               self.ui(theme, ui);
            });
         });
      self.open = open;
      new_theme
   }

   /// Show the ui for the theme editor
   pub fn ui(&mut self, theme: &mut Theme, ui: &mut Ui) {
      ui.vertical_centered(|ui| {
         ui.spacing_mut().item_spacing.y = 10.0;

         CollapsingHeader::new("Theme Frames").show(ui, |ui| {
            CollapsingHeader::new("Native Window Frame").show(ui, |ui| {
               self.frame_settings(
                  &mut theme.window_frame,
                  &mut theme.frame1_visuals,
                  ui,
               );
            });

            CollapsingHeader::new("Frame 1").show(ui, |ui| {
               self.frame_settings(&mut theme.frame1, &mut theme.frame1_visuals, ui);
            });

            CollapsingHeader::new("Frame 2").show(ui, |ui| {
               self.frame_settings(&mut theme.frame2, &mut theme.frame2_visuals, ui);
            });
         });

         CollapsingHeader::new("Theme Colors").show(ui, |ui| {
            ui.label("Background Color");
            color_edit_button_srgba(ui, &mut theme.colors.bg_color, Alpha::OnlyBlend);

            ui.label("Extreme Background Color");
            color_edit_button_srgba(
               ui,
               &mut theme.colors.extreme_bg_color,
               Alpha::OnlyBlend,
            );

            ui.label("Window Fill");
            color_edit_button_srgba(
               ui,
               &mut theme.colors.window_fill,
               Alpha::OnlyBlend,
            );

            ui.label("Highlight1 Color");
            color_edit_button_srgba(ui, &mut theme.colors.highlight1, Alpha::OnlyBlend);

            ui.label("Highlight2 Color");
            color_edit_button_srgba(ui, &mut theme.colors.highlight2, Alpha::OnlyBlend);

            ui.label("Overlay Color");
            color_edit_button_srgba(
               ui,
               &mut theme.colors.overlay_color,
               Alpha::OnlyBlend,
            );

            ui.label("Text Color");
            color_edit_button_srgba(ui, &mut theme.colors.text_color, Alpha::OnlyBlend);

            ui.label("Text Secondary Color");
            color_edit_button_srgba(
               ui,
               &mut theme.colors.text_secondary,
               Alpha::OnlyBlend,
            );

            ui.label("Error Color");
            color_edit_button_srgba(
               ui,
               &mut theme.colors.error_color,
               Alpha::OnlyBlend,
            );

            ui.label("Success Color");
            color_edit_button_srgba(
               ui,
               &mut theme.colors.success_color,
               Alpha::OnlyBlend,
            );

            ui.label("Hyperlink Color");
            color_edit_button_srgba(
               ui,
               &mut theme.colors.hyperlink_color,
               Alpha::OnlyBlend,
            );

            ui.label("Text Edit Background Color");
            color_edit_button_srgba(
               ui,
               &mut theme.colors.text_edit_bg,
               Alpha::OnlyBlend,
            );

            ui.label("Button Background Color");
            color_edit_button_srgba(ui, &mut theme.colors.button_bg, Alpha::OnlyBlend);

            ui.label("Widget Bg Color");
            color_edit_button_srgba(
               ui,
               &mut theme.colors.widget_bg_color,
               Alpha::OnlyBlend,
            );

            ui.label("Widget Bg Color on click");
            color_edit_button_srgba(
               ui,
               &mut theme.colors.widget_bg_color_click,
               Alpha::OnlyBlend,
            );

            ui.label("Widget Bg Color on hover");
            color_edit_button_srgba(
               ui,
               &mut theme.colors.widget_bg_color_hover,
               Alpha::OnlyBlend,
            );

            ui.label("Widget Bg Color on open");
            color_edit_button_srgba(
               ui,
               &mut theme.colors.widget_bg_color_open,
               Alpha::OnlyBlend,
            );

            ui.label("Border Color");
            color_edit_button_srgba(
               ui,
               &mut theme.colors.border_color_idle,
               Alpha::OnlyBlend,
            );

            ui.label("Border Color on click");
            color_edit_button_srgba(
               ui,
               &mut theme.colors.border_color_click,
               Alpha::OnlyBlend,
            );

            ui.label("Border Color on hover");
            color_edit_button_srgba(
               ui,
               &mut theme.colors.border_color_hover,
               Alpha::OnlyBlend,
            );

            ui.label("Border Color on open");
            color_edit_button_srgba(
               ui,
               &mut theme.colors.border_color_open,
               Alpha::OnlyBlend,
            );
         });

         CollapsingHeader::new("Text Sizes").show(ui, |ui| {
            ui.label("Very Small");
            ui.add(Slider::new(&mut theme.text_sizes.very_small, 0.0..=100.0).text("Size"));

            ui.label("Small");
            ui.add(Slider::new(&mut theme.text_sizes.small, 0.0..=100.0).text("Size"));

            ui.label("Normal");
            ui.add(Slider::new(&mut theme.text_sizes.normal, 0.0..=100.0).text("Size"));

            ui.label("Large");
            ui.add(Slider::new(&mut theme.text_sizes.large, 0.0..=100.0).text("Size"));

            ui.label("Very Large");
            ui.add(Slider::new(&mut theme.text_sizes.very_large, 0.0..=100.0).text("Size"));

            ui.label("Heading");
            ui.add(Slider::new(&mut theme.text_sizes.heading, 0.0..=100.0).text("Size"));
         });

         CollapsingHeader::new("Other Colors").show(ui, |ui| {
            ui.label("Selection Stroke");
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.selection.stroke.width,
                  0.0..=10.0,
               )
               .text("Stroke Width"),
            );
            ui.label("Selection Stroke Color");
            color_edit_button_srgba(
               ui,
               &mut theme.style.visuals.selection.stroke.color,
               Alpha::OnlyBlend,
            );
            ui.label("Selection Bg Fill");
            color_edit_button_srgba(
               ui,
               &mut theme.style.visuals.selection.bg_fill,
               Alpha::OnlyBlend,
            );

            ui.label("Hyperlink Color");
            color_edit_button_srgba(
               ui,
               &mut theme.style.visuals.hyperlink_color,
               Alpha::OnlyBlend,
            );

            ui.label("Faint Background Color");
            color_edit_button_srgba(
               ui,
               &mut theme.style.visuals.faint_bg_color,
               Alpha::OnlyBlend,
            );

            ui.label("Extreme Background Color");
            color_edit_button_srgba(
               ui,
               &mut theme.style.visuals.extreme_bg_color,
               Alpha::OnlyBlend,
            );

            ui.label("Code Background Color");
            color_edit_button_srgba(
               ui,
               &mut theme.style.visuals.code_bg_color,
               Alpha::OnlyBlend,
            );

            ui.label("Warning Text Color");
            color_edit_button_srgba(
               ui,
               &mut theme.style.visuals.warn_fg_color,
               Alpha::OnlyBlend,
            );

            ui.label("Error Text Color");
            color_edit_button_srgba(
               ui,
               &mut theme.style.visuals.error_fg_color,
               Alpha::OnlyBlend,
            );

            ui.label("Panel Fill Color");
            color_edit_button_srgba(
               ui,
               &mut theme.style.visuals.panel_fill,
               Alpha::OnlyBlend,
            );
         });

         CollapsingHeader::new("Window Visuals").show(ui, |ui| {
            ui.label("Window Rounding");
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.window_corner_radius.nw,
                  0..=35,
               )
               .text("Top Left"),
            );
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.window_corner_radius.ne,
                  0..=35,
               )
               .text("Top Right"),
            );
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.window_corner_radius.sw,
                  0..=35,
               )
               .text("Bottom Left"),
            );
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.window_corner_radius.se,
                  0..=35,
               )
               .text("Bottom Right"),
            );

            ui.label("Window Shadow");
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.window_shadow.offset[0],
                  -100..=100,
               )
               .text("Offset X"),
            );
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.window_shadow.offset[1],
                  -100..=100,
               )
               .text("Offset Y"),
            );
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.window_shadow.blur,
                  0..=100,
               )
               .text("Blur"),
            );
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.window_shadow.spread,
                  0..=100,
               )
               .text("Spread"),
            );
            ui.label("Shadow Color");
            color_edit_button_srgba(
               ui,
               &mut theme.style.visuals.window_shadow.color,
               Alpha::OnlyBlend,
            );

            ui.label("Window Fill Color");
            color_edit_button_srgba(
               ui,
               &mut theme.style.visuals.window_fill,
               Alpha::OnlyBlend,
            );

            ui.label("Window Stroke Color");
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.window_stroke.width,
                  0.0..=10.0,
               )
               .text("Stroke Width"),
            );
            color_edit_button_srgba(
               ui,
               &mut theme.style.visuals.window_stroke.color,
               Alpha::OnlyBlend,
            );

            ui.label("Window Highlight Topmost");
            ui.checkbox(
               &mut theme.style.visuals.window_highlight_topmost,
               "Highlight Topmost",
            );
         });

         CollapsingHeader::new("Popup Shadow").show(ui, |ui| {
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.popup_shadow.offset[0],
                  -100..=100,
               )
               .text("Offset X"),
            );
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.popup_shadow.offset[1],
                  -100..=100,
               )
               .text("Offset Y"),
            );
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.popup_shadow.blur,
                  0..=100,
               )
               .text("Blur"),
            );
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.popup_shadow.spread,
                  0..=100,
               )
               .text("Spread"),
            );
            color_edit_button_srgba(
               ui,
               &mut theme.style.visuals.popup_shadow.color,
               Alpha::OnlyBlend,
            );
         });

         CollapsingHeader::new("Menu Rounding").show(ui, |ui| {
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.menu_corner_radius.nw,
                  0..=35,
               )
               .text("Top Left"),
            );
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.menu_corner_radius.ne,
                  0..=35,
               )
               .text("Top Right"),
            );
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.menu_corner_radius.sw,
                  0..=35,
               )
               .text("Bottom Left"),
            );
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.menu_corner_radius.se,
                  0..=35,
               )
               .text("Bottom Right"),
            );
         });

         CollapsingHeader::new("Widget Visuals").show(ui, |ui| {
            self.select_widget_state(ui);

            let widget_visuals = match self.widget_state {
               WidgetState::NonInteractive => &mut theme.style.visuals.widgets.noninteractive,
               WidgetState::Inactive => &mut theme.style.visuals.widgets.inactive,
               WidgetState::Hovered => &mut theme.style.visuals.widgets.hovered,
               WidgetState::Active => &mut theme.style.visuals.widgets.active,
               WidgetState::Open => &mut theme.style.visuals.widgets.open,
            };

            ui.label("Background Fill Color");
            color_edit_button_srgba(ui, &mut widget_visuals.bg_fill, Alpha::OnlyBlend);

            ui.label("Weak Background Fill Color");
            color_edit_button_srgba(
               ui,
               &mut widget_visuals.weak_bg_fill,
               Alpha::OnlyBlend,
            );

            ui.label("Background Stroke Color");
            ui.add(
               Slider::new(&mut widget_visuals.bg_stroke.width, 0.0..=10.0).text("Stroke Width"),
            );
            color_edit_button_srgba(
               ui,
               &mut widget_visuals.bg_stroke.color,
               Alpha::OnlyBlend,
            );

            ui.label("Rounding");
            ui.add(Slider::new(&mut widget_visuals.corner_radius.nw, 0..=255).text("Top Left"));
            ui.add(Slider::new(&mut widget_visuals.corner_radius.ne, 0..=255).text("Top Right"));
            ui.add(Slider::new(&mut widget_visuals.corner_radius.sw, 0..=255).text("Bottom Left"));
            ui.add(Slider::new(&mut widget_visuals.corner_radius.se, 0..=255).text("Bottom Right"));

            ui.label("Foreground Stroke Color");
            ui.add(
               Slider::new(&mut widget_visuals.fg_stroke.width, 0.0..=10.0).text("Stroke Width"),
            );
            color_edit_button_srgba(
               ui,
               &mut widget_visuals.fg_stroke.color,
               Alpha::OnlyBlend,
            );

            ui.label("Expansion");
            ui.add(Slider::new(&mut widget_visuals.expansion, 0.0..=100.0).text("Expansion"));
         });

         CollapsingHeader::new("Other Settings").show(ui, |ui| {
            ui.label("Resize Corner Size");
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.resize_corner_size,
                  0.0..=100.0,
               )
               .text("Corner Size"),
            );

            ui.label("Button Frame");
            ui.checkbox(
               &mut theme.style.visuals.button_frame,
               "Button Frame",
            );
         });
      });
   }

   fn frame_settings(&mut self, frame: &mut Frame, visuals: &mut FrameVisuals, ui: &mut Ui) {
      CollapsingHeader::new("Inner & Outter Margin").show(ui, |ui| {
         ui.label("Inner Margin");
         ui.add(Slider::new(&mut frame.inner_margin.top, 0..=127).text("Top"));
         ui.add(Slider::new(&mut frame.inner_margin.bottom, 0..=127).text("Bottom"));
         ui.add(Slider::new(&mut frame.inner_margin.left, 0..=127).text("Left"));
         ui.add(Slider::new(&mut frame.inner_margin.right, 0..=127).text("Right"));

         ui.label("Outter Margin");
         ui.add(Slider::new(&mut frame.outer_margin.top, 0..=127).text("Top"));
         ui.add(Slider::new(&mut frame.outer_margin.bottom, 0..=127).text("Bottom"));
         ui.add(Slider::new(&mut frame.outer_margin.left, 0..=127).text("Left"));
         ui.add(Slider::new(&mut frame.outer_margin.right, 0..=127).text("Right"));
      });

      CollapsingHeader::new("Visuals").show(ui, |ui| {
         self.frame_visuals(visuals, ui);
      });

      ui.label("Rounding");
      ui.add(Slider::new(&mut frame.corner_radius.nw, 0..=255).text("Top Left"));
      ui.add(Slider::new(&mut frame.corner_radius.ne, 0..=255).text("Top Right"));
      ui.add(Slider::new(&mut frame.corner_radius.sw, 0..=255).text("Bottom Left"));
      ui.add(Slider::new(&mut frame.corner_radius.se, 0..=255).text("Bottom Right"));

      ui.label("Shadow");
      ui.add(Slider::new(&mut frame.shadow.offset[0], -128..=127).text("Offset X"));
      ui.add(Slider::new(&mut frame.shadow.offset[1], -128..=127).text("Offset Y"));
      ui.add(Slider::new(&mut frame.shadow.blur, 0..=255).text("Blur"));
      ui.add(Slider::new(&mut frame.shadow.spread, 0..=255).text("Spread"));
      ui.label("Shadow Color");
      color_edit_button_srgba(ui, &mut frame.shadow.color, Alpha::OnlyBlend);

      ui.label("Fill Color");
      color_edit_button_srgba(ui, &mut frame.fill, Alpha::OnlyBlend);

      ui.label("Stroke Width & Color");
      ui.add(Slider::new(&mut frame.stroke.width, 0.0..=100.0).text("Stroke Width"));
      color_edit_button_srgba(ui, &mut frame.stroke.color, Alpha::OnlyBlend);
   }

   fn frame_visuals(&mut self, visuals: &mut FrameVisuals, ui: &mut Ui) {
      ui.label("Background Color on hover");
      color_edit_button_srgba(ui, &mut visuals.bg_on_hover, Alpha::OnlyBlend);

      ui.label("Background Color on click");
      color_edit_button_srgba(ui, &mut visuals.bg_on_click, Alpha::OnlyBlend);

      ui.label("Border Color on hover");
      color_edit_button_srgba(
         ui,
         &mut visuals.border_on_hover.1,
         Alpha::OnlyBlend,
      );
      ui.add(
         Slider::new(&mut visuals.border_on_hover.0, 0.0..=100.0).text("Border Width on hover"),
      );

      ui.label("Border Color on click");
      color_edit_button_srgba(
         ui,
         &mut visuals.border_on_click.1,
         Alpha::OnlyBlend,
      );
      ui.add(
         Slider::new(&mut visuals.border_on_click.0, 0.0..=100.0).text("Border Width on click"),
      );
   }

   fn select_widget_state(&mut self, ui: &mut Ui) {
      ComboBox::from_label("")
         .selected_text(self.widget_state.to_str())
         .show_ui(ui, |ui| {
            for widget in self.widget_state.to_vec() {
               let value = ui.selectable_value(
                  &mut self.widget_state,
                  widget.clone(),
                  widget.to_str(),
               );

               if value.clicked() {
                  self.widget_state = widget;
               }
            }
         });
   }
}
