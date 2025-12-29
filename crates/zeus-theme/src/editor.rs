use egui::{
   Align, Button, CollapsingHeader, Color32, ComboBox, CornerRadius, DragValue, Frame, Layout, Margin, Order, Popup, PopupCloseBehavior, Rect, Response, RichText, ScrollArea, Sense, SetOpenCommand, Shadow, Slider, Stroke, StrokeKind, TextEdit, Ui, Vec2, Window, color_picker::{Alpha, color_edit_button_srgba}, ecolor::HexColor, vec2
};

use super::{Theme, hsla::Hsla, utils};
use crate::{ButtonVisuals, ComboBoxVisuals, TextEditVisuals, ThemeColors};

/// Identify which state of the widget we should edit
#[derive(Clone, PartialEq)]
pub enum WidgetState {
   NonInteractive,
   Inactive,
   Hovered,
   Active,
   Open,
}

#[derive(Clone, PartialEq)]
pub enum Color {
   Bg(Color32),
   WidgetBG(Color32),
   Hover(Color32),
   Text(Color32),
   TextMuted(Color32),
   Highlight(Color32),
   Border(Color32),
   Accent(Color32),
   Error(Color32),
   Warning(Color32),
   Success(Color32),
   Info(Color32),
}

impl Color {
   pub fn all_colors_from(theme: &ThemeColors) -> Vec<Color> {
      vec![
         Color::Bg(theme.bg),
         Color::WidgetBG(theme.widget_bg),
         Color::Hover(theme.hover),
         Color::Text(theme.text),
         Color::TextMuted(theme.text_muted),
         Color::Highlight(theme.highlight),
         Color::Border(theme.border),
         Color::Accent(theme.accent),
         Color::Error(theme.error),
         Color::Warning(theme.warning),
         Color::Success(theme.success),
         Color::Info(theme.info),
      ]
   }

   pub fn to_str(&self) -> &'static str {
      match self {
         Color::Bg(_) => "Bg",
         Color::WidgetBG(_) => "WidgetBG",
         Color::Hover(_) => "Hover",
         Color::Text(_) => "Text",
         Color::TextMuted(_) => "Text Muted",
         Color::Highlight(_) => "Highlight",
         Color::Border(_) => "Border",
         Color::Accent(_) => "Accent",
         Color::Error(_) => "Error",
         Color::Warning(_) => "Warning",
         Color::Success(_) => "Success",
         Color::Info(_) => "Info",
      }
   }

   pub fn color32(&self) -> Color32 {
      match self {
         Color::Bg(color) => *color,
         Color::WidgetBG(color) => *color,
         Color::Hover(color) => *color,
         Color::Text(color) => *color,
         Color::TextMuted(color) => *color,
         Color::Highlight(color) => *color,
         Color::Border(color) => *color,
         Color::Accent(color) => *color,
         Color::Error(color) => *color,
         Color::Warning(color) => *color,
         Color::Success(color) => *color,
         Color::Info(color) => *color,
      }
   }

   pub fn name_from(color: Color32, theme_colors: &ThemeColors) -> &'static str {
      if color == theme_colors.bg {
         "Bg"
      } else if color == theme_colors.widget_bg {
         "WidgetBG"
      } else if color == theme_colors.hover {
         "Hover"
      } else if color == theme_colors.text {
         "Text"
      } else if color == theme_colors.text_muted {
         "Text Muted"
      } else if color == theme_colors.highlight {
         "Highlight"
      } else if color == theme_colors.border {
         "Border"
      } else if color == theme_colors.accent {
         "Accent"
      } else if color == theme_colors.error {
         "Error"
      } else if color == theme_colors.warning {
         "Warning"
      } else if color == theme_colors.success {
         "Success"
      } else if color == theme_colors.info {
         "Info"
      } else {
         "Unknown"
      }
   }
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
   pub hsla_edit_button: HslaEditButton,
   pub color: Color,
   pub bg_color: Color32,
   pub size: (f32, f32),
}

impl ThemeEditor {
   pub fn new() -> Self {
      Self {
         open: false,
         widget_state: WidgetState::NonInteractive,
         hsla_edit_button: HslaEditButton::new(),
         color: Color::Bg(Color32::TRANSPARENT),
         bg_color: Color32::from_rgba_premultiplied(32, 45, 70, 255),
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
      let frame = Frame::window(ui.style()).fill(self.bg_color);

      Window::new("Theme Editor")
         .open(&mut open)
         .resizable([true, true])
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_min_width(self.size.0);
            ui.set_min_height(self.size.1);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);
            ui.style_mut().visuals = super::themes::dark::theme().style.visuals.clone();

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
         let colors = theme.colors.clone();

         CollapsingHeader::new("Theme Frames").show(ui, |ui| {
            CollapsingHeader::new("Native Window Frame").show(ui, |ui| {
               self.frame_settings(&mut theme.window_frame, ui);
            });

            CollapsingHeader::new("Frame 1").show(ui, |ui| {
               self.frame_settings(&mut theme.frame1, ui);
            });

            CollapsingHeader::new("Frame 2").show(ui, |ui| {
               self.frame_settings(&mut theme.frame2, ui);
            });
         });

         CollapsingHeader::new("Custom Widgets Visuals").show(ui, |ui| {
            CollapsingHeader::new("Button").show(ui, |ui| {
               CollapsingHeader::new("Button Visuals 1").show(ui, |ui| {
                  self.button_visuals(colors, &mut theme.colors.button_visuals, ui);
               });

            });

            CollapsingHeader::new("Label").show(ui, |ui| {
               CollapsingHeader::new("Label Visuals 1").show(ui, |ui| {
                  self.button_visuals(colors, &mut theme.colors.label_visuals, ui);
               });

            });

            CollapsingHeader::new("Combo Box").show(ui, |ui| {
               CollapsingHeader::new("Combo Box Visuals 1").show(ui, |ui| {
                  self.combo_box_visuals(colors, &mut theme.colors.combo_box_visuals, ui);
               });

            });

            CollapsingHeader::new("Text Edit").show(ui, |ui| {
               CollapsingHeader::new("Text Edit Visuals 1").show(ui, |ui| {
                  self.text_edit_visuals(colors, &mut theme.colors.text_edit_visuals, ui);
               });
            });
         });

         CollapsingHeader::new("Theme Colors").show(ui, |ui| {
            ui.label("BG");
            self.hsla_edit_button.show("bg", ui, &mut theme.colors.bg);

            ui.label("WidgetBG");
            self.hsla_edit_button.show("widgetbg", ui, &mut theme.colors.widget_bg);

            ui.label("Hover");
            self.hsla_edit_button.show("hover", ui, &mut theme.colors.hover);

            ui.label("Text");
            self.hsla_edit_button.show("text1", ui, &mut theme.colors.text);

            ui.label("Text Muted");
            self.hsla_edit_button.show("text_muted1", ui, &mut theme.colors.text_muted);

            ui.label("Highlight");
            self.hsla_edit_button.show("highlight1", ui, &mut theme.colors.highlight);

            ui.label("Border");
            self.hsla_edit_button.show("border1", ui, &mut theme.colors.border);

            ui.label("Accent");
            self.hsla_edit_button.show("accent", ui, &mut theme.colors.accent);

            ui.label("Error");
            self.hsla_edit_button.show("error1", ui, &mut theme.colors.error);

            ui.label("Warning");
            self.hsla_edit_button.show("warning1", ui, &mut theme.colors.warning);

            ui.label("Success");
            self.hsla_edit_button.show("success1", ui, &mut theme.colors.success);

            ui.label("Info");
            self.hsla_edit_button.show("info1", ui, &mut theme.colors.info);
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
            self.hsla_edit_button.show(
               "selection_stroke_color1",
               ui,
               &mut theme.style.visuals.selection.stroke.color,
            );

            ui.label("Selection Bg Fill");
            self.hsla_edit_button.show(
               "selection_bg_fill1",
               ui,
               &mut theme.style.visuals.selection.bg_fill,
            );

            ui.label("Hyperlink Color");
            self.hsla_edit_button.show(
               "hyperlink_color1",
               ui,
               &mut theme.style.visuals.hyperlink_color,
            );

            ui.label("Faint Background Color");
            self.hsla_edit_button.show(
               "faint_bg_color1",
               ui,
               &mut theme.style.visuals.faint_bg_color,
            );

            ui.label("Extreme Background Color");
            self.hsla_edit_button.show(
               "extreme_bg_color1",
               ui,
               &mut theme.style.visuals.extreme_bg_color,
            );

            ui.label("Code Background Color");
            self.hsla_edit_button.show(
               "code_bg_color1",
               ui,
               &mut theme.style.visuals.code_bg_color,
            );

            ui.label("Warning Text Color");
            self.hsla_edit_button.show(
               "warn_fg_color1",
               ui,
               &mut theme.style.visuals.warn_fg_color,
            );

            ui.label("Error Text Color");
            self.hsla_edit_button.show(
               "error_fg_color1",
               ui,
               &mut theme.style.visuals.error_fg_color,
            );

            ui.label("Panel Fill Color");
            self.hsla_edit_button.show(
               "panel_fill1",
               ui,
               &mut theme.style.visuals.panel_fill,
            );
         });

         CollapsingHeader::new("Window Visuals").show(ui, |ui| {
            ui.label("Window Rounding");
            edit_corner_radius(&mut theme.style.visuals.window_corner_radius, ui);

            ui.label("Window Shadow");
            edit_shadow(&mut theme.style.visuals.window_shadow, ui);

            ui.label("Window Fill Color");
            self.hsla_edit_button.show(
               "window_fill1",
               ui,
               &mut theme.style.visuals.window_fill,
            );

            ui.label("Window Stroke Color");
            edit_stroke(&mut theme.style.visuals.window_stroke, ui);

            ui.label("Window Highlight Topmost");
            ui.checkbox(
               &mut theme.style.visuals.window_highlight_topmost,
               "Highlight Topmost",
            );
         });

         CollapsingHeader::new("Popup Shadow").show(ui, |ui| {
            edit_shadow(&mut theme.style.visuals.popup_shadow, ui);
         });

         CollapsingHeader::new("Menu Rounding").show(ui, |ui| {
            edit_corner_radius(&mut theme.style.visuals.menu_corner_radius, ui);
         });

         CollapsingHeader::new("Widget Visuals").show(ui, |ui| {
            self.widget_settings(theme, ui);
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

         CollapsingHeader::new("Tessellation").show(ui, |ui| {
            self.tesellation_settings(theme, ui);
         });
      });
   }

   fn tesellation_settings(&mut self, theme: &Theme, ui: &mut Ui) {
      let text_size = theme.text_sizes.normal;

      let mut options = ui.ctx().tessellation_options(|options| options.clone());

      let text = RichText::new("Feathering").size(text_size);

      ui.checkbox(&mut options.feathering, text);

      ui.add(
         DragValue::new(&mut options.feathering_size_in_pixels)
            .speed(0.1)
            .range(0.0..=100.0),
      );

      let text = RichText::new("Coarse tessellation culling").size(text_size);
      ui.checkbox(&mut options.coarse_tessellation_culling, text);

      let text = RichText::new("Precomputed discs").size(text_size);
      ui.checkbox(&mut options.prerasterized_discs, text);

      let text = RichText::new("Round text to pixels").size(text_size);
      ui.checkbox(&mut options.round_text_to_pixels, text);

      let text = RichText::new("Round line segments to pixels").size(text_size);
      ui.checkbox(&mut options.round_line_segments_to_pixels, text);

      let text = RichText::new("Round rects to pixels").size(text_size);
      ui.checkbox(&mut options.round_rects_to_pixels, text);

      let text = RichText::new("Debug paint text rects").size(text_size);
      ui.checkbox(&mut options.debug_paint_text_rects, text);

      let text = RichText::new("Debug paint clip rects").size(text_size);
      ui.checkbox(&mut options.debug_paint_clip_rects, text);

      let text = RichText::new("Debug ignore clip rects").size(text_size);
      ui.checkbox(&mut options.debug_ignore_clip_rects, text);

      let text = RichText::new("Bezier tolerance").size(text_size);
      ui.label(text);
      ui.add(DragValue::new(&mut options.bezier_tolerance).speed(0.1).range(0.0..=1.0));

      let text = RichText::new("Epsilon").size(text_size);
      ui.label(text);
      ui.add(DragValue::new(&mut options.epsilon).speed(0.1).range(0.0..=1.0));

      let text = RichText::new("Parallel tessellation").size(text_size);
      ui.checkbox(&mut options.parallel_tessellation, text);

      let text = RichText::new("Validate meshes").size(text_size);
      ui.checkbox(&mut options.validate_meshes, text);

      ui.ctx().tessellation_options_mut(|options_mut| {
         *options_mut = options;
      });
   }

   fn button_visuals(&mut self, colors: ThemeColors, visuals: &mut ButtonVisuals, ui: &mut Ui) {
      let text = RichText::new("Button Visuals");
      ui.label(text);

      ui.label("Text Color");
      ui.horizontal(|ui| {
         let color = self.color_select("1", visuals.text, &colors, ui);
         if let Some(color) = color {
            visuals.text = color.color32();
         }

         self.hsla_edit_button.show("text1", ui, &mut visuals.text);
      });

      ui.label("Background Color");
      ui.horizontal(|ui| {
         let color = self.color_select("2", visuals.bg, &colors, ui);
         if let Some(color) = color {
            visuals.bg = color.color32();
         }

         self.hsla_edit_button.show("bg1", ui, &mut visuals.bg);
      });

      ui.label("Background Hover Color");
      ui.horizontal(|ui| {
         let color = self.color_select("3", visuals.bg_hover, &colors, ui);
         if let Some(color) = color {
            visuals.bg_hover = color.color32();
         }

         self.hsla_edit_button.show("bg_hover1", ui, &mut visuals.bg_hover);
      });

      ui.label("Background Click Color");
      ui.horizontal(|ui| {
         let color = self.color_select("4", visuals.bg_click, &colors, ui);
         if let Some(color) = color {
            visuals.bg_click = color.color32();
         }

         self.hsla_edit_button.show("bg_click1", ui, &mut visuals.bg_click);
      });

      ui.label("Background Selected");
      ui.horizontal(|ui| {
         let color = self.color_select("5", visuals.bg_selected, &colors, ui);

         if let Some(color) = color {
            visuals.bg_selected = color.color32();
         }

         self.hsla_edit_button.show("bg_selected1", ui, &mut visuals.bg_selected);
      });

      ui.label("Border Color");
      ui.horizontal(|ui| {
         let color = self.color_select("6", visuals.border.color, &colors, ui);

         if let Some(color) = color {
            visuals.border.color = color.color32();
         }

         self.hsla_edit_button.show("border1", ui, &mut visuals.border.color);
      });

      ui.label("Border Hover Color");
      ui.horizontal(|ui| {
         let color = self.color_select("7", visuals.border_hover.color, &colors, ui);
         if let Some(color) = color {
            visuals.border_hover.color = color.color32();
         }

         self.hsla_edit_button.show(
            "border_hover1",
            ui,
            &mut visuals.border_hover.color,
         );
      });

      ui.label("Border Click Color");
      ui.horizontal(|ui| {
         let color = self.color_select("8", visuals.border_click.color, &colors, ui);
         if let Some(color) = color {
            visuals.border_click.color = color.color32();
         }

         self.hsla_edit_button.show(
            "border_click1",
            ui,
            &mut visuals.border_click.color,
         );
      });

      //  ui.label("Corner Radius");
      //  ui.add(Slider::new(&mut visuals.corner_radius, 0.0..=25.0).text("Corner Radius"));

      ui.label("Shadow");
      ui.horizontal(|ui| {
         let color = self.color_select("9", visuals.shadow.color, &colors, ui);
         if let Some(color) = color {
            visuals.shadow.color = color.color32();
         }

         /*
         self.hsla_edit_button.show(
            "shadow_color1",
            ui,
            &mut colors.button_visuals.shadow.color,
         );
         */

         color_edit_button_srgba(
            ui,
            &mut visuals.shadow.color,
            Alpha::BlendOrAdditive,
         );
      });

      ui.label("Shadow Offset");
      ui.add(Slider::new(&mut visuals.shadow.offset[0], -100..=100).text("Offset X"));
      ui.add(Slider::new(&mut visuals.shadow.offset[1], -100..=100).text("Offset Y"));

      ui.label("Shadow Blur");
      ui.add(Slider::new(&mut visuals.shadow.blur, 0..=100).text("Blur"));

      ui.label("Shadow Spread");
      ui.add(Slider::new(&mut visuals.shadow.spread, 0..=100).text("Spread"));
   }

   fn combo_box_visuals(
      &mut self,
      colors: ThemeColors,
      visuals: &mut ComboBoxVisuals,
      ui: &mut Ui,
   ) {
      ui.label("Background Color");
      ui.horizontal(|ui| {
         let color = self.color_select("2", visuals.bg, &colors, ui);
         if let Some(color) = color {
            visuals.bg = color.color32();
         }

         self.hsla_edit_button.show("bg1", ui, &mut visuals.bg);
      });

      ui.label("Background Hover Color");
      ui.horizontal(|ui| {
         let color = self.color_select("3", visuals.bg_hover, &colors, ui);
         if let Some(color) = color {
            visuals.bg_hover = color.color32();
         }

         self.hsla_edit_button.show("bg_hover1", ui, &mut visuals.bg_hover);
      });

      ui.label("Border Color");
      ui.horizontal(|ui| {
         let color = self.color_select("4", visuals.border.color, &colors, ui);

         if let Some(color) = color {
            visuals.border.color = color.color32();
         }

         self.hsla_edit_button.show("border1", ui, &mut visuals.border.color);
      });

      ui.label("Border Hover Color");
      ui.horizontal(|ui| {
         let color = self.color_select("5", visuals.border_hover.color, &colors, ui);
         if let Some(color) = color {
            visuals.border_hover.color = color.color32();
         }

         self.hsla_edit_button.show(
            "border_hover1",
            ui,
            &mut visuals.border_hover.color,
         );
      });

      ui.label("Border Open Color");
      ui.horizontal(|ui| {
         let color = self.color_select("6", visuals.border_open.color, &colors, ui);
         if let Some(color) = color {
            visuals.border_open.color = color.color32();
         }

         self.hsla_edit_button.show("border_open1", ui, &mut visuals.border_open.color);
      });

      //  ui.label("Corner Radius");
      //  ui.add(Slider::new(&mut visuals.corner_radius, 0.0..=25.0).text("Corner Radius"));

      ui.label("Shadow");
      ui.horizontal(|ui| {
         let color = self.color_select("9", visuals.shadow.color, &colors, ui);
         if let Some(color) = color {
            visuals.shadow.color = color.color32();
         }

         color_edit_button_srgba(
            ui,
            &mut visuals.shadow.color,
            Alpha::BlendOrAdditive,
         );
      });

      ui.label("Shadow Offset");
      ui.add(Slider::new(&mut visuals.shadow.offset[0], -100..=100).text("Offset X"));
      ui.add(Slider::new(&mut visuals.shadow.offset[1], -100..=100).text("Offset Y"));

      ui.label("Shadow Blur");
      ui.add(Slider::new(&mut visuals.shadow.blur, 0..=100).text("Blur"));

      ui.label("Shadow Spread");
      ui.add(Slider::new(&mut visuals.shadow.spread, 0..=100).text("Spread"));
   }

   fn text_edit_visuals(
      &mut self,
      colors: ThemeColors,
      visuals: &mut TextEditVisuals,
      ui: &mut Ui,
   ) {
      ui.label("Text Color");
      ui.horizontal(|ui| {
         let color = self.color_select("1", visuals.text, &colors, ui);
         if let Some(color) = color {
            visuals.text = color.color32();
         }

         self.hsla_edit_button.show("text1", ui, &mut visuals.text);
      });

      ui.label("Background Color");
      ui.horizontal(|ui| {
         let color = self.color_select("2", visuals.bg, &colors, ui);
         if let Some(color) = color {
            visuals.bg = color.color32();
         }

         self.hsla_edit_button.show("bg1", ui, &mut visuals.bg);
      });

      ui.label("Border Color");
      ui.horizontal(|ui| {
         let color = self.color_select("3", visuals.border.color, &colors, ui);

         if let Some(color) = color {
            visuals.border.color = color.color32();
         }

         self.hsla_edit_button.show("border1", ui, &mut visuals.border.color);
      });

      ui.label("Border Hover Color");
      ui.horizontal(|ui| {
         let color = self.color_select("4", visuals.border_hover.color, &colors, ui);
         if let Some(color) = color {
            visuals.border_hover.color = color.color32();
         }

         self.hsla_edit_button.show(
            "border_hover1",
            ui,
            &mut visuals.border_hover.color,
         );
      });

      ui.label("Border Open Color");
      ui.horizontal(|ui| {
         let color = self.color_select("5", visuals.border_open.color, &colors, ui);
         if let Some(color) = color {
            visuals.border_open.color = color.color32();
         }

         self.hsla_edit_button.show("border_open1", ui, &mut visuals.border_open.color);
      });

      //  ui.label("Corner Radius");
      //  ui.add(Slider::new(&mut visuals.corner_radius, 0.0..=25.0).text("Corner Radius"));

      ui.label("Shadow");
      ui.horizontal(|ui| {
         let color = self.color_select("6", visuals.shadow.color, &colors, ui);
         if let Some(color) = color {
            visuals.shadow.color = color.color32();
         }

         color_edit_button_srgba(
            ui,
            &mut visuals.shadow.color,
            Alpha::BlendOrAdditive,
         );
      });

      ui.label("Shadow Offset");
      ui.add(Slider::new(&mut visuals.shadow.offset[0], -100..=100).text("Offset X"));
      ui.add(Slider::new(&mut visuals.shadow.offset[1], -100..=100).text("Offset Y"));

      ui.label("Shadow Blur");
      ui.add(Slider::new(&mut visuals.shadow.blur, 0..=100).text("Blur"));

      ui.label("Shadow Spread");
      ui.add(Slider::new(&mut visuals.shadow.spread, 0..=100).text("Spread"));
   }

   fn widget_settings(&mut self, theme: &mut Theme, ui: &mut Ui) {
      self.select_widget_state(ui);

      let widget_visuals = match self.widget_state {
         WidgetState::NonInteractive => &mut theme.style.visuals.widgets.noninteractive,
         WidgetState::Inactive => &mut theme.style.visuals.widgets.inactive,
         WidgetState::Hovered => &mut theme.style.visuals.widgets.hovered,
         WidgetState::Active => &mut theme.style.visuals.widgets.active,
         WidgetState::Open => &mut theme.style.visuals.widgets.open,
      };

      ui.label("Background Fill Color");

      ui.horizontal(|ui| {
         let color = self.color_select("1", widget_visuals.bg_fill, &theme.colors, ui);
         if let Some(color) = color {
            widget_visuals.bg_fill = color.color32();
         }

         self.hsla_edit_button.show("bg_fill1", ui, &mut widget_visuals.bg_fill);
      });

      ui.label("Weak Background Fill Color");

      ui.horizontal(|ui| {
         let color = self.color_select(
            "2",
            widget_visuals.weak_bg_fill,
            &theme.colors,
            ui,
         );
         if let Some(color) = color {
            widget_visuals.weak_bg_fill = color.color32();
         }

         self.hsla_edit_button.show(
            "weak_bg_fill1",
            ui,
            &mut widget_visuals.weak_bg_fill,
         );
      });

      ui.label("Background Stroke Width");
      ui.add(Slider::new(
         &mut widget_visuals.bg_stroke.width,
         0.0..=10.0,
      ));

      ui.label("Background Stroke Color");
      ui.horizontal(|ui| {
         let color = self.color_select(
            "3",
            widget_visuals.bg_stroke.color,
            &theme.colors,
            ui,
         );
         if let Some(color) = color {
            widget_visuals.bg_stroke.color = color.color32();
         }

         self.hsla_edit_button.show(
            "bg_stroke_color1",
            ui,
            &mut widget_visuals.bg_stroke.color,
         );
      });

      ui.label("Rounding");
      edit_corner_radius(&mut widget_visuals.corner_radius, ui);

      ui.label("Foreground Stroke Width");
      ui.add(Slider::new(
         &mut widget_visuals.fg_stroke.width,
         0.0..=10.0,
      ));

      ui.label("Foreground Stroke Color");
      ui.horizontal(|ui| {
         let color = self.color_select(
            "4",
            widget_visuals.fg_stroke.color,
            &theme.colors,
            ui,
         );

         if let Some(color) = color {
            widget_visuals.fg_stroke.color = color.color32();
         }

         self.hsla_edit_button.show(
            "fg_stroke_color1",
            ui,
            &mut widget_visuals.fg_stroke.color,
         );
      });

      ui.label("Expansion");
      ui.add(Slider::new(&mut widget_visuals.expansion, 0.0..=100.0).text("Expansion"));
   }

   fn frame_settings(&mut self, frame: &mut Frame, ui: &mut Ui) {
      CollapsingHeader::new("Inner & Outter Margin").show(ui, |ui| {
         ui.label("Inner Margin");
         edit_margin(&mut frame.inner_margin, ui);

         ui.label("Outter Margin");
         edit_margin(&mut frame.outer_margin, ui);
      });

      ui.label("Rounding");
      edit_corner_radius(&mut frame.corner_radius, ui);

      ui.label("Shadow");
      edit_shadow(&mut frame.shadow, ui);

      ui.label("Fill Color");
      self.hsla_edit_button.show("fill_color1", ui, &mut frame.fill);

      ui.label("Stroke Width & Color");
      edit_stroke(&mut frame.stroke, ui);
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

   fn color_select(
      &mut self,
      id: &str,
      current_color: Color32,
      colors: &ThemeColors,
      ui: &mut Ui,
   ) -> Option<Color> {
      let all_colors = Color::all_colors_from(colors);

      let mut selected_color = None;
      let current_color_name = Color::name_from(current_color, colors);

      ComboBox::from_id_salt(id).selected_text(current_color_name).show_ui(ui, |ui| {
         for color in all_colors {
            let value = ui.selectable_value(&mut self.color, color.clone(), color.to_str());

            if value.clicked() {
               selected_color = Some(color);
            }
         }
      });
      selected_color
   }
}

fn edit_stroke(stroke: &mut Stroke, ui: &mut Ui) {
   ui.add(Slider::new(&mut stroke.width, 0.0..=100.0).text("Stroke Width"));

   ui.label("Stroke Color");
   color_edit_button_srgba(ui, &mut stroke.color, Alpha::BlendOrAdditive);
}

fn edit_margin(margin: &mut Margin, ui: &mut Ui) {
   ui.add(Slider::new(&mut margin.top, 0..=127).text("Top"));
   ui.add(Slider::new(&mut margin.bottom, 0..=127).text("Bottom"));
   ui.add(Slider::new(&mut margin.left, 0..=127).text("Left"));
   ui.add(Slider::new(&mut margin.right, 0..=127).text("Right"));
}

fn edit_corner_radius(corner_radius: &mut CornerRadius, ui: &mut Ui) {
   ui.add(Slider::new(&mut corner_radius.nw, 0..=255).text("Top Left"));
   ui.add(Slider::new(&mut corner_radius.ne, 0..=255).text("Top Right"));
   ui.add(Slider::new(&mut corner_radius.sw, 0..=255).text("Bottom Left"));
   ui.add(Slider::new(&mut corner_radius.se, 0..=255).text("Bottom Right"));
}

fn edit_shadow(shadow: &mut Shadow, ui: &mut Ui) {
   ui.add(Slider::new(&mut shadow.offset[0], -128..=127).text("Offset X"));
   ui.add(Slider::new(&mut shadow.offset[1], -128..=127).text("Offset Y"));
   ui.add(Slider::new(&mut shadow.blur, 0..=255).text("Blur"));
   ui.add(Slider::new(&mut shadow.spread, 0..=255).text("Spread"));

   ui.label("Shadow Color");
   color_edit_button_srgba(ui, &mut shadow.color, Alpha::BlendOrAdditive);
}

#[derive(Clone)]
pub struct HslaEditButton {
   from_hex_text: String,
}

impl HslaEditButton {
   pub fn new() -> Self {
      Self {
         from_hex_text: String::new(),
      }
   }

   pub fn show(&mut self, id: &str, ui: &mut Ui, color32: &mut Color32) -> Response {
      let stroke = Stroke::new(1.0, Color32::GRAY);
      let button_size = Vec2::new(50.0, 20.0);
      let (rect, mut response) = ui.allocate_exact_size(button_size, Sense::click());
      ui.painter().rect_filled(rect, 4.0, *color32);
      ui.painter().rect_stroke(rect, 4.0, stroke, StrokeKind::Inside);

      let popup_id = ui.make_persistent_id(id);

      let set_command = if response.clicked() {
         Some(SetOpenCommand::Toggle)
      } else {
         None
      };

      let close_behavior = PopupCloseBehavior::CloseOnClickOutside;
      response.layer_id.order = Order::Debug;

      let popup = Popup::from_response(&response)
         .close_behavior(close_behavior)
         .open_memory(set_command);

      let working_id = popup_id.with("working_hsla");
      let mut working_hsla = ui
         .memory(|mem| mem.data.get_temp(working_id))
         .unwrap_or_else(|| Hsla::from_color32(*color32));

      let popup_res = popup.show(|ui| self.hsla_picker_ui(ui, &mut working_hsla));

      if let Some(inner) = popup_res {
         // if color changed
         if inner.inner {
            ui.memory_mut(|mem| mem.data.insert_temp(working_id, working_hsla));
            *color32 = working_hsla.to_color32();
            response.mark_changed();
         }
      } else {
         ui.memory_mut(|mem| mem.data.remove::<Hsla>(working_id));
      }

      response
   }

   // The core HSLA picker UI (sliders, 2D square, preview). Returns true if changed.
   fn hsla_picker_ui(&mut self, ui: &mut Ui, hsla: &mut Hsla) -> bool {
      let mut changed = false;
      let stroke = Stroke::new(1.0, Color32::GRAY);

      ui.horizontal(|ui| {
         ui.set_width(200.0);

         // Left: 2D S-L square + hue slider below it
         ui.vertical(|ui| {
            changed |= sl_2d_picker(ui, hsla);
            changed |= hue_slider(ui, hsla);
            changed |= alpha_slider(ui, hsla);
         });

         // Right: Preview + numeric controls
         ui.vertical(|ui| {
            // Preview rect
            let preview_size = Vec2::new(80.0, 80.0);
            let (rect, _) = ui.allocate_exact_size(preview_size, Sense::hover());
            ui.painter().rect_filled(rect, 4.0, hsla.to_color32());
            ui.painter().rect_stroke(rect, 4.0, stroke, StrokeKind::Inside);

            ui.label(RichText::new("Preview").strong());

            // Numeric sliders for precision
            ui.add_space(10.0);
            changed |= ui.add(Slider::new(&mut hsla.h, 0.0..=360.0).text("Hue")).changed();
            changed |= ui.add(Slider::new(&mut hsla.s, 0.0..=100.0).text("Saturation")).changed();
            changed |= ui.add(Slider::new(&mut hsla.l, 0.0..=100.0).text("Lightness")).changed();
            changed |= ui.add(Slider::new(&mut hsla.a, 0.0..=1.0).text("Alpha")).changed();
         });

         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.vertical(|ui| {
               // RGBA copy button
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let (r, g, b, a) = hsla.to_rgba_components();
                  let text = RichText::new(format!("RGBA ({r}, {g}, {b}, {a})"));
                  let button = Button::new(text).min_size(vec2(160.0, 15.0));
                  if ui.add(button).clicked() {
                     ui.ctx().copy_text(format!("({r}, {g}, {b}, {a})"));
                  }
               });

               // HEX copy button
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let hex_color = HexColor::Hex6(hsla.to_color32());
                  let text = RichText::new(format!("HEX {}", hex_color));
                  let button = Button::new(text).min_size(vec2(160.0, 15.0));
                  if ui.add(button).clicked() {
                     ui.ctx().copy_text(format!("{}", hex_color));
                  }
               });

               // From RBG to HSLA
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let text = RichText::new("Convert From HEX");
                  let button = Button::new(text).small();
                  ui.add(TextEdit::singleline(&mut self.from_hex_text));
                  if ui.add(button).clicked() {
                     let new_color = Hsla::from_hex(&self.from_hex_text);
                     if let Some(new_color) = new_color {
                        *hsla = new_color;
                        changed = true;
                     }
                  }
               });
            });
         });
      });

      changed
   }
}

// 2D picker for Saturation (x) and Lightness (y)
fn sl_2d_picker(ui: &mut Ui, hsla: &mut Hsla) -> bool {
   let size = Vec2::new(150.0, 150.0);
   let (rect, response) = ui.allocate_exact_size(size, Sense::drag());

   let mut changed = false;

   if response.dragged() {
      if let Some(pos) = response.hover_pos() {
         let relative = pos - rect.min;
         hsla.s = (relative.x / size.x).clamp(0.0, 1.0) * 100.0;
         hsla.l = (1.0 - (relative.y / size.y)).clamp(0.0, 1.0) * 100.0; // Top: high L, bottom: low L
         changed = true;
      }
   }

   // Paint gradient background (grid of small rects for simplicity)
   let painter = ui.painter();
   const RES: usize = 64; // Higher for smoother, but 64 is fast and looks good
   let cell_size = size / RES as f32;
   for i in 0..RES {
      for j in 0..RES {
         let s = (i as f32 / (RES - 1) as f32) * 100.0;
         let l = (1.0 - (j as f32 / (RES - 1) as f32)) * 100.0; // Top: l=100, bottom: l=0
         let temp_hsla = Hsla {
            h: hsla.h,
            s,
            l,
            a: 1.0,
         };
         let color = temp_hsla.to_color32();

         let min = rect.min + Vec2::new(i as f32 * cell_size.x, j as f32 * cell_size.y);
         let cell_rect = Rect::from_min_size(min, cell_size);
         painter.rect_filled(cell_rect, 0.0, color);
      }
   }

   // Draw cursor at current position
   let x = (hsla.s / 100.0) * size.x;
   let y = (1.0 - hsla.l / 100.0) * size.y;
   let cursor_pos = rect.min + Vec2::new(x, y);
   painter.circle_stroke(cursor_pos, 5.0, Stroke::new(1.0, Color32::WHITE));
   painter.circle_stroke(cursor_pos, 5.0, Stroke::new(1.0, Color32::BLACK));

   // Outline the square
   painter.rect_stroke(
      rect,
      0.0,
      Stroke::new(1.0, Color32::GRAY),
      StrokeKind::Inside,
   );

   changed
}

// Hue gradient slider
fn hue_slider(ui: &mut Ui, hsla: &mut Hsla) -> bool {
   let size = Vec2::new(150.0, 20.0);
   let (rect, response) = ui.allocate_exact_size(size, Sense::drag());

   let mut changed = false;

   if response.dragged() {
      if let Some(pos) = response.hover_pos() {
         let relative_x = (pos.x - rect.min.x) / size.x;
         hsla.h = relative_x.clamp(0.0, 1.0) * 360.0;
         changed = true;
      }
   }

   // Paint rainbow gradient
   let painter = ui.painter();
   const RES: usize = 128; // Smooth horizontal gradient
   let cell_width = size.x / RES as f32;
   for i in 0..RES {
      let h = (i as f32 / (RES - 1) as f32) * 360.0;
      let temp_hsla = Hsla {
         h,
         s: 100.0,
         l: 50.0,
         a: 1.0,
      }; // Full sat, mid light for vibrant rainbow
      let color = temp_hsla.to_color32();

      let min = rect.min + Vec2::new(i as f32 * cell_width, 0.0);
      let cell_rect = Rect::from_min_size(min, Vec2::new(cell_width, size.y));
      painter.rect_filled(cell_rect, 0.0, color);
   }

   // Cursor indicator (vertical line)
   let x = (hsla.h / 360.0) * size.x;
   let line_start = rect.min + Vec2::new(x, 0.0);
   let line_end = rect.min + Vec2::new(x, size.y);
   painter.line_segment(
      [line_start, line_end],
      Stroke::new(2.0, Color32::WHITE),
   );

   // Outline
   painter.rect_stroke(
      rect,
      4.0,
      Stroke::new(1.0, Color32::GRAY),
      StrokeKind::Inside,
   );

   changed
}

fn alpha_slider(ui: &mut Ui, hsla: &mut Hsla) -> bool {
   let size = Vec2::new(150.0, 20.0);
   let (rect, response) = ui.allocate_exact_size(size, Sense::drag());
   let mut changed = false;

   if response.dragged() {
      if let Some(pos) = response.hover_pos() {
         let relative_x = (pos.x - rect.min.x) / size.x;
         hsla.a = relative_x.clamp(0.0, 1.0);
         changed = true;
      }
   }

   let painter = ui.painter();
   // Paint checkerboard FIRST for transparency visibility
   let checker_size = 5.0;

   for x in (0..=((size.x / checker_size) as usize)).step_by(1) {
      for y in (0..=((size.y / checker_size) as usize)).step_by(1) {
         let color = if (x + y) % 2 == 0 {
            Color32::GRAY
         } else {
            Color32::LIGHT_GRAY
         };
         let min = rect.min + Vec2::new(x as f32 * checker_size, y as f32 * checker_size);
         let cell_rect = Rect::from_min_size(min, Vec2::splat(checker_size)).intersect(rect);
         painter.rect_filled(cell_rect, 0.0, color);
      }
   }

   // Then paint gradient on top
   const RES: usize = 64;
   let cell_width = size.x / RES as f32;

   for i in 0..RES {
      let a = i as f32 / (RES - 1) as f32;
      let temp_hsla = Hsla {
         h: hsla.h,
         s: hsla.s,
         l: hsla.l,
         a,
      };

      let color = temp_hsla.to_color32();
      let min = rect.min + Vec2::new(i as f32 * cell_width, 0.0);
      let cell_rect = Rect::from_min_size(min, Vec2::new(cell_width, size.y));
      painter.rect_filled(cell_rect, 0.0, color);
   }

   // Cursor line
   let x = hsla.a * size.x;
   let line_start = rect.min + Vec2::new(x, 0.0);
   let line_end = rect.min + Vec2::new(x, size.y);

   painter.line_segment(
      [line_start, line_end],
      Stroke::new(2.0, Color32::WHITE),
   );

   // Outline
   painter.rect_stroke(
      rect,
      4.0,
      Stroke::new(1.0, Color32::GRAY),
      StrokeKind::Inside,
   );
   changed
}
