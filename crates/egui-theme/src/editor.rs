use egui::{
   ecolor::HexColor, vec2, Align, Button, CollapsingHeader, Color32, ComboBox, Frame, Layout, Popup, PopupCloseBehavior, Rect, Response, RichText, ScrollArea, Sense, SetOpenCommand, Slider, Stroke, StrokeKind, Ui, Vec2, Window
};

use super::{Theme, hsla::Hsla, utils};
use crate::ThemeColors;

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
   BgDark(Color32),
   Bg(Color32),
   BgLight(Color32),
   Text(Color32),
   TextMuted(Color32),
   Highlight(Color32),
   Border(Color32),
   Primary(Color32),
   Secondary(Color32),
   Error(Color32),
   Warning(Color32),
   Success(Color32),
   Info(Color32),
}

impl Color {
   pub fn all_colors_from(theme: &ThemeColors) -> Vec<Color> {
      vec![
         Color::BgDark(theme.bg_dark),
         Color::Bg(theme.bg),
         Color::BgLight(theme.bg_light),
         Color::Text(theme.text),
         Color::TextMuted(theme.text_muted),
         Color::Highlight(theme.highlight),
         Color::Border(theme.border),
         Color::Primary(theme.primary),
         Color::Secondary(theme.secondary),
         Color::Error(theme.error),
         Color::Warning(theme.warning),
         Color::Success(theme.success),
         Color::Info(theme.info),
      ]
   }

   pub fn to_str(&self) -> &'static str {
      match self {
         Color::BgDark(_) => "Bg Dark",
         Color::Bg(_) => "Bg",
         Color::BgLight(_) => "Bg Light",
         Color::Text(_) => "Text",
         Color::TextMuted(_) => "Text Muted",
         Color::Highlight(_) => "Highlight",
         Color::Border(_) => "Border",
         Color::Primary(_) => "Primary",
         Color::Secondary(_) => "Secondary",
         Color::Error(_) => "Error",
         Color::Warning(_) => "Warning",
         Color::Success(_) => "Success",
         Color::Info(_) => "Info",
      }
   }

   pub fn color32(&self) -> Color32 {
      match self {
         Color::BgDark(color) => *color,
         Color::Bg(color) => *color,
         Color::BgLight(color) => *color,
         Color::Text(color) => *color,
         Color::TextMuted(color) => *color,
         Color::Highlight(color) => *color,
         Color::Border(color) => *color,
         Color::Primary(color) => *color,
         Color::Secondary(color) => *color,
         Color::Error(color) => *color,
         Color::Warning(color) => *color,
         Color::Success(color) => *color,
         Color::Info(color) => *color,
      }
   }

   pub fn name_from(color: Color32, theme_colors: &ThemeColors) -> &'static str {
      if color == theme_colors.bg_dark {
         "Bg Dark"
      } else if color == theme_colors.bg {
         "Bg"
      } else if color == theme_colors.bg_light {
         "Bg Light"
      } else if color == theme_colors.text {
         "Text"
      } else if color == theme_colors.text_muted {
         "Text Muted"
      } else if color == theme_colors.highlight {
         "Highlight"
      } else if color == theme_colors.border {
         "Border"
      } else if color == theme_colors.primary {
         "Primary"
      } else if color == theme_colors.secondary {
         "Secondary"
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
   pub color: Color,
   pub bg_color: Color32,
   pub size: (f32, f32),
}

impl ThemeEditor {
   pub fn new() -> Self {
      Self {
         open: false,
         widget_state: WidgetState::NonInteractive,
         color: Color::BgDark(Color32::TRANSPARENT),
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
               self.frame_settings(&mut theme.window_frame, ui);
            });

            CollapsingHeader::new("Frame 1").show(ui, |ui| {
               self.frame_settings(&mut theme.frame1, ui);
            });

            CollapsingHeader::new("Frame 2").show(ui, |ui| {
               self.frame_settings(&mut theme.frame2, ui);
            });
         });

         CollapsingHeader::new("Theme Colors").show(ui, |ui| {
            ui.label("BG Dark");
            hsla_edit_button("bg_dark1", ui, &mut theme.colors.bg_dark);

            ui.label("BG");
            hsla_edit_button("bg1", ui, &mut theme.colors.bg);

            ui.label("BG Light");
            hsla_edit_button("bg_light1", ui, &mut theme.colors.bg_light);

            ui.label("BG Light 2");
            hsla_edit_button("bg_light2", ui, &mut theme.colors.bg_light2);

            ui.label("Text");
            hsla_edit_button("text1", ui, &mut theme.colors.text);

            ui.label("Text Muted");
            hsla_edit_button("text_muted1", ui, &mut theme.colors.text_muted);

            ui.label("Highlight");
            hsla_edit_button("highlight1", ui, &mut theme.colors.highlight);

            ui.label("Border");
            hsla_edit_button("border1", ui, &mut theme.colors.border);

            ui.label("Primary");
            hsla_edit_button("primary1", ui, &mut theme.colors.primary);

            ui.label("Secondary");
            hsla_edit_button("secondary1", ui, &mut theme.colors.secondary);

            ui.label("Error");
            hsla_edit_button("error1", ui, &mut theme.colors.error);

            ui.label("Warning");
            hsla_edit_button("warning1", ui, &mut theme.colors.warning);

            ui.label("Success");
            hsla_edit_button("success1", ui, &mut theme.colors.success);

            ui.label("Info");
            hsla_edit_button("info1", ui, &mut theme.colors.info);
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
            hsla_edit_button(
               "selection_stroke_color1",
               ui,
               &mut theme.style.visuals.selection.stroke.color,
            );

            ui.label("Selection Bg Fill");
            hsla_edit_button(
               "selection_bg_fill1",
               ui,
               &mut theme.style.visuals.selection.bg_fill,
            );

            ui.label("Hyperlink Color");
            hsla_edit_button(
               "hyperlink_color1",
               ui,
               &mut theme.style.visuals.hyperlink_color,
            );

            ui.label("Faint Background Color");
            hsla_edit_button(
               "faint_bg_color1",
               ui,
               &mut theme.style.visuals.faint_bg_color,
            );

            ui.label("Extreme Background Color");
            hsla_edit_button(
               "extreme_bg_color1",
               ui,
               &mut theme.style.visuals.extreme_bg_color,
            );

            ui.label("Code Background Color");
            hsla_edit_button(
               "code_bg_color1",
               ui,
               &mut theme.style.visuals.code_bg_color,
            );

            ui.label("Warning Text Color");
            hsla_edit_button(
               "warn_fg_color1",
               ui,
               &mut theme.style.visuals.warn_fg_color,
            );

            ui.label("Error Text Color");
            hsla_edit_button(
               "error_fg_color1",
               ui,
               &mut theme.style.visuals.error_fg_color,
            );

            ui.label("Panel Fill Color");
            hsla_edit_button(
               "panel_fill1",
               ui,
               &mut theme.style.visuals.panel_fill,
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
            hsla_edit_button(
               "window_shadow_color1",
               ui,
               &mut theme.style.visuals.window_shadow.color,
            );

            ui.label("Window Fill Color");
            hsla_edit_button(
               "window_fill1",
               ui,
               &mut theme.style.visuals.window_fill,
            );

            ui.label("Window Stroke Color");
            ui.add(
               Slider::new(
                  &mut theme.style.visuals.window_stroke.width,
                  0.0..=10.0,
               )
               .text("Stroke Width"),
            );

            hsla_edit_button(
               "window_stroke_color1",
               ui,
               &mut theme.style.visuals.window_stroke.color,
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

            hsla_edit_button(
               "popup_shadow_color1",
               ui,
               &mut theme.style.visuals.popup_shadow.color,
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
      });
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

         hsla_edit_button("bg_fill1", ui, &mut widget_visuals.bg_fill);
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

         hsla_edit_button(
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

         hsla_edit_button(
            "bg_stroke_color1",
            ui,
            &mut widget_visuals.bg_stroke.color,
         );
      });

      ui.label("Rounding");
      ui.add(Slider::new(&mut widget_visuals.corner_radius.nw, 0..=255).text("Top Left"));
      ui.add(Slider::new(&mut widget_visuals.corner_radius.ne, 0..=255).text("Top Right"));
      ui.add(Slider::new(&mut widget_visuals.corner_radius.sw, 0..=255).text("Bottom Left"));
      ui.add(Slider::new(&mut widget_visuals.corner_radius.se, 0..=255).text("Bottom Right"));

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

         hsla_edit_button(
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
      hsla_edit_button("shadow_color1", ui, &mut frame.shadow.color);

      ui.label("Fill Color");
      hsla_edit_button("fill_color1", ui, &mut frame.fill);

      ui.label("Stroke Width & Color");
      ui.add(Slider::new(&mut frame.stroke.width, 0.0..=100.0).text("Stroke Width"));
      hsla_edit_button("stroke_color1", ui, &mut frame.stroke.color);
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

pub fn hsla_edit_button(id: &str, ui: &mut Ui, color32: &mut Color32) -> Response {
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
   let popup = Popup::from_response(&response)
      .close_behavior(close_behavior)
      .open_memory(set_command);

   let working_id = popup_id.with("working_hsla");
   let mut working_hsla = ui
      .memory(|mem| mem.data.get_temp(working_id))
      .unwrap_or_else(|| Hsla::from_color32(*color32));

   let popup_res = popup.show(|ui| hsla_picker_ui(ui, &mut working_hsla));

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
fn hsla_picker_ui(ui: &mut Ui, hsla: &mut Hsla) -> bool {
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
         });
      });
   });

   changed
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
