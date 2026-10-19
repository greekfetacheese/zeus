//! Settings → Appearance: hold-to-confirm and the completion burst.

use crate::core::ZeusContext;
use crate::core::types::{
   AppearanceSettings, MAX_BRANCH_DEPTH, MAX_BURST_BOLTS, MAX_BURST_INTENSITY, MAX_BURST_MS,
   MAX_GLOW_FROM, MAX_GLOW_SOFTNESS, MAX_GLOW_TO, MAX_HOLD_SECONDS, MAX_JITTER, MAX_THICKNESS,
   MIN_BRANCH_DEPTH, MIN_BURST_BOLTS, MIN_BURST_INTENSITY, MIN_BURST_MS, MIN_GLOW_FROM,
   MIN_GLOW_SOFTNESS, MIN_GLOW_TO, MIN_HOLD_SECONDS, MIN_JITTER, MIN_THICKNESS,
   SECOND_COLOUR_ALPHA, accent_at,
};
use crate::utils::RT;
use egui::{Align, Color32, Layout, RichText, Sense, Ui, vec2};
use egui_elements::{Button, ComboBox, HoldBurst, HoldSettings, Label, Theme};
use elegance::{Badge, BadgeTone};

/// Fixed width of a row's label column, in points.
const LABEL_W: f32 = 150.0;
/// Fixed width of a row's control column, in points.
const CONTROL_W: f32 = 200.0;
/// Fixed width of a row's value cell, in points.
const VALUE_W: f32 = 64.0;
/// Width shared by the preview and Reset buttons, in points.
const PREVIEW_BUTTON_W: f32 = 220.0;
/// Height shared by the preview and Reset buttons, in points.
const PREVIEW_BUTTON_H: f32 = 32.0;

const ENABLE_TIP: &str = "Require a hold on Confirm and Sign. Reject and Cancel stay instant. \
                          Off by default, and the sweep plus the burst only appear while it is on.";
const DURATION_TIP: &str = "How long the affirmative button must be held before it fires. \
                            A misclick is released long before this.";
const ACCENT_TIP: &str = "Sweep with the theme's accent colour instead of a colour of your own.";
const FILL_FROM_TIP: &str = "Colour the hold's sweep starts at.";
const SHIFT_TIP: &str = "Shift the sweep to a second colour as the hold completes. On by default; \
                         turn it off for the crate's solid fill.";
const END_COLOUR_TIP: &str = "Colour the sweep ends at. The default is the theme accent at 80 \
                              alpha, so it fades to about a third opacity; pick a colour to \
                              override it.";
const BURST_TIP: &str = "Effect played when the hold completes. Off paints nothing extra.";
const BURST_COLOR_TIP: &str = "Colour of the completion burst. Off uses the fill colour at \
                               completion, so the burst matches the sweep.";
const INTENSITY_TIP: &str = "Strength of the completion burst. 0 hides it without changing the \
                             kind; 1 is the effect's own profile.";
const BURST_MS_TIP: &str = "How long the completion burst lasts, in milliseconds.";
const BOLTS_TIP: &str = "How many lightning bolts strike across the button.";
const DEPTH_TIP: &str = "How many times each bolt branches. 0 draws the trunk only.";
const THICKNESS_TIP: &str = "Thickness of the lightning's core stroke, in pixels.";
const JITTER_TIP: &str = "How far the bolt wanders between segments, as a fraction of each \
                          segment's length. Higher is wilder.";
const GLOW_FROM_TIP: &str = "Starting radius of the glow, as a multiple of the button's shorter \
                             side.";
const GLOW_TO_TIP: &str = "Ending radius of the glow, as a multiple of the button's shorter side. \
                           Kept at or above the starting radius.";
const GLOW_SOFTNESS_TIP: &str = "How soft the glow's falloff is. Values above 1 spread the colour \
                                 further; values below 1 tighten the hot core.";

pub struct AppearanceSettingsUi {
   /// A slider drag reports `changed()` every frame; the file write waits until
   /// the pointer is released so one drag is one write.
   dirty: bool,
   /// Whether the preview's most recent hold completed.
   preview_confirmed: bool,
}

impl AppearanceSettingsUi {
   pub fn new(ctx: &mut ZeusContext) -> Self {
      let mut this = Self {
         dirty: false,
         preview_confirmed: false,
      };
      this.sync_from_ctx(ctx);
      this
   }

   /// The UI reads `ctx.appearance_settings` directly, so the only local state to
   /// reset is the write lag and the preview. Clamping here means a hand-edited
   /// file cannot show a value its slider cannot represent.
   pub fn sync_from_ctx(&mut self, ctx: &mut ZeusContext) {
      ctx.appearance_settings.clamp();
      self.dirty = false;
      self.preview_confirmed = false;
   }

   fn persist(ctx: &ZeusContext) {
      let settings = ctx.appearance_settings;
      RT.spawn_blocking(move || {
         if let Err(e) = settings.save() {
            tracing::error!("Failed to save appearance settings: {e}");
         }
      });
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      ui.spacing_mut().item_spacing = vec2(theme.spacing.xs, theme.spacing.md);
      ui.spacing_mut().button_padding = theme.button_padding;

      let button_visuals = theme.button_visuals();
      let q_mark_text = RichText::new("?").size(theme.typography.normal);
      let combo_visuals = theme.combo_box_visuals();
      let label_visuals = theme.label_visuals();

      ui.label(RichText::new("Hold to Confirm").size(theme.typography.very_large));

      let mut enabled = ctx.appearance_settings.hold_to_confirm();
      let mut changed = false;
      Self::row(
         ui,
         theme,
         &q_mark_text,
         ENABLE_TIP,
         "Hold to confirm",
         None,
         |ui| {
            changed = ui.checkbox(&mut enabled, "").changed();
         },
      );
      if changed {
         ctx.appearance_settings.set_hold_to_confirm(enabled);
         self.dirty = true;
      }

      ui.add_enabled_ui(enabled, |ui| {
         let mut seconds = ctx.appearance_settings.hold_seconds();
         let value = format!("{seconds:.2} s");
         let mut changed = false;
         Self::row(
            ui,
            theme,
            &q_mark_text,
            DURATION_TIP,
            "Duration",
            Some(&value),
            |ui| {
               changed = ui
                  .add(
                     egui::Slider::new(&mut seconds, MIN_HOLD_SECONDS..=MAX_HOLD_SECONDS)
                        .step_by(0.05)
                        .show_value(false),
                  )
                  .changed();
            },
         );
         if changed {
            ctx.appearance_settings.set_hold_seconds(seconds);
            self.dirty = true;
         }

         ui.add_space(theme.spacing.xl);
         ui.label(RichText::new("Fill").size(theme.typography.very_large));

         let mut use_accent = ctx.appearance_settings.hold.fill_from.is_none();
         let mut changed = false;
         Self::row(
            ui,
            theme,
            &q_mark_text,
            ACCENT_TIP,
            "Theme accent",
            None,
            |ui| {
               changed = ui.checkbox(&mut use_accent, "").changed();
            },
         );
         if changed {
            ctx.appearance_settings.hold.fill_from = if use_accent {
               None
            } else {
               Some(theme.colors.accent)
            };
            self.dirty = true;
         }

         if !use_accent {
            let mut rgb =
               srgb(ctx.appearance_settings.hold.fill_from.unwrap_or(theme.colors.accent));
            let mut changed = false;
            Self::row(
               ui,
               theme,
               &q_mark_text,
               FILL_FROM_TIP,
               "Start colour",
               None,
               |ui| {
                  changed = ui.color_edit_button_srgb(&mut rgb).changed();
               },
            );
            if changed {
               ctx.appearance_settings.hold.fill_from =
                  Some(Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
               self.dirty = true;
            }
         }

         let mut shift = ctx.appearance_settings.shift_second_colour;
         let mut changed = false;
         Self::row(
            ui,
            theme,
            &q_mark_text,
            SHIFT_TIP,
            "Second colour",
            None,
            |ui| {
               changed = ui.checkbox(&mut shift, "").changed();
            },
         );
         if changed {
            ctx.appearance_settings.shift_second_colour = shift;
            self.dirty = true;
         }

         if shift {
            let end_colour = ctx
               .appearance_settings
               .second_colour
               .unwrap_or_else(|| accent_at(theme.colors.accent, SECOND_COLOUR_ALPHA));
            let mut rgba = end_colour.to_srgba_unmultiplied();
            let mut changed = false;
            Self::row(
               ui,
               theme,
               &q_mark_text,
               END_COLOUR_TIP,
               "End colour",
               None,
               |ui| {
                  changed = ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed();
               },
            );
            if changed {
               ctx.appearance_settings.second_colour = Some(Color32::from_rgba_unmultiplied(
                  rgba[0], rgba[1], rgba[2], rgba[3],
               ));
               self.dirty = true;
            }
         }

         ui.add_space(theme.spacing.xl);
         ui.label(RichText::new("Burst").size(theme.typography.very_large));

         let mut chosen = ctx.appearance_settings.hold.burst;
         let selected_text = RichText::new(chosen.to_str()).size(theme.typography.normal);
         let kind_label = Label::new(selected_text, None)
            .visuals(label_visuals)
            .sense(Sense::click())
            .expand(Some(6.0))
            .interactive(true)
            .fill_width(true);
         let mut changed = false;
         Self::row(
            ui,
            theme,
            &q_mark_text,
            BURST_TIP,
            "Kind",
            None,
            |ui| {
               ComboBox::new("appearance_burst_kind", kind_label)
                  .width(CONTROL_W)
                  .visuals(combo_visuals)
                  .show_ui(ui, |ui| {
                     ui.spacing_mut().item_spacing.y = theme.spacing.sm;

                     for kind in [HoldBurst::Off, HoldBurst::Glow, HoldBurst::Lightning] {
                        let text = RichText::new(kind.to_str()).size(theme.typography.normal);
                        let option = Label::new(text, None)
                           .visuals(label_visuals)
                           .expand(Some(6.0))
                           .sense(Sense::click())
                           .interactive(true)
                           .fill_width(true);

                        if ui.add(option).clicked() {
                           chosen = kind;
                        }
                     }
                  });
               changed = chosen != ctx.appearance_settings.hold.burst;
            },
         );
         if changed {
            ctx.appearance_settings.hold.burst = chosen;
            self.dirty = true;
         }

         let mut same_color = ctx.appearance_settings.hold.burst_color.is_none();
         let mut changed = false;
         Self::row(
            ui,
            theme,
            &q_mark_text,
            BURST_COLOR_TIP,
            "Same as fill",
            None,
            |ui| {
               changed = ui.checkbox(&mut same_color, "").changed();
            },
         );
         if changed {
            ctx.appearance_settings.hold.burst_color = if same_color {
               None
            } else {
               Some(
                  ctx.appearance_settings
                     .resolved_hold(theme.colors.accent)
                     .fill_to
                     .or(ctx.appearance_settings.hold.fill_from)
                     .unwrap_or(theme.colors.accent),
               )
            };
            self.dirty = true;
         }

         if !same_color {
            let mut rgb =
               srgb(ctx.appearance_settings.hold.burst_color.unwrap_or(theme.colors.accent));
            let mut changed = false;
            Self::row(
               ui,
               theme,
               &q_mark_text,
               BURST_COLOR_TIP,
               "Burst colour",
               None,
               |ui| {
                  changed = ui.color_edit_button_srgb(&mut rgb).changed();
               },
            );
            if changed {
               ctx.appearance_settings.hold.burst_color =
                  Some(Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
               self.dirty = true;
            }
         }

         let value = format!(
            "{:.2}",
            ctx.appearance_settings.hold.burst_intensity
         );
         let mut changed = false;
         Self::row(
            ui,
            theme,
            &q_mark_text,
            INTENSITY_TIP,
            "Intensity",
            Some(&value),
            |ui| {
               changed = ui
                  .add(
                     egui::Slider::new(
                        &mut ctx.appearance_settings.hold.burst_intensity,
                        MIN_BURST_INTENSITY..=MAX_BURST_INTENSITY,
                     )
                     .step_by(0.05)
                     .show_value(false),
                  )
                  .changed();
            },
         );
         if changed {
            ctx.appearance_settings.clamp();
            self.dirty = true;
         }

         let value = format!("{:.0} ms", ctx.appearance_settings.hold.burst_ms);
         let mut changed = false;
         Self::row(
            ui,
            theme,
            &q_mark_text,
            BURST_MS_TIP,
            "Burst length",
            Some(&value),
            |ui| {
               changed = ui
                  .add(
                     egui::Slider::new(
                        &mut ctx.appearance_settings.hold.burst_ms,
                        MIN_BURST_MS..=MAX_BURST_MS,
                     )
                     .step_by(10.0)
                     .show_value(false),
                  )
                  .changed();
            },
         );
         if changed {
            ctx.appearance_settings.clamp();
            self.dirty = true;
         }

         if ctx.appearance_settings.hold.burst == HoldBurst::Lightning {
            let value = format!("{}", ctx.appearance_settings.hold.burst_bolts);
            let mut changed = false;
            Self::row(
               ui,
               theme,
               &q_mark_text,
               BOLTS_TIP,
               "Bolts",
               Some(&value),
               |ui| {
                  changed = ui
                     .add(
                        egui::Slider::new(
                           &mut ctx.appearance_settings.hold.burst_bolts,
                           MIN_BURST_BOLTS..=MAX_BURST_BOLTS,
                        )
                        .show_value(false),
                     )
                     .changed();
               },
            );
            if changed {
               ctx.appearance_settings.clamp();
               self.dirty = true;
            }

            let value = format!(
               "{}",
               ctx.appearance_settings.hold.burst_branch_depth
            );
            let mut changed = false;
            Self::row(
               ui,
               theme,
               &q_mark_text,
               DEPTH_TIP,
               "Branch depth",
               Some(&value),
               |ui| {
                  changed = ui
                     .add(
                        egui::Slider::new(
                           &mut ctx.appearance_settings.hold.burst_branch_depth,
                           MIN_BRANCH_DEPTH..=MAX_BRANCH_DEPTH,
                        )
                        .show_value(false),
                     )
                     .changed();
               },
            );
            if changed {
               ctx.appearance_settings.clamp();
               self.dirty = true;
            }

            let value = format!(
               "{:.1}",
               ctx.appearance_settings.hold.burst_thickness
            );
            let mut changed = false;
            Self::row(
               ui,
               theme,
               &q_mark_text,
               THICKNESS_TIP,
               "Thickness",
               Some(&value),
               |ui| {
                  changed = ui
                     .add(
                        egui::Slider::new(
                           &mut ctx.appearance_settings.hold.burst_thickness,
                           MIN_THICKNESS..=MAX_THICKNESS,
                        )
                        .step_by(0.1)
                        .show_value(false),
                     )
                     .changed();
               },
            );
            if changed {
               ctx.appearance_settings.clamp();
               self.dirty = true;
            }

            let value = format!("{:.3}", ctx.appearance_settings.hold.burst_jitter);
            let mut changed = false;
            Self::row(
               ui,
               theme,
               &q_mark_text,
               JITTER_TIP,
               "Jitter",
               Some(&value),
               |ui| {
                  changed = ui
                     .add(
                        egui::Slider::new(
                           &mut ctx.appearance_settings.hold.burst_jitter,
                           MIN_JITTER..=MAX_JITTER,
                        )
                        .step_by(0.01)
                        .show_value(false),
                     )
                     .changed();
               },
            );
            if changed {
               ctx.appearance_settings.clamp();
               self.dirty = true;
            }
         }

         if ctx.appearance_settings.hold.burst == HoldBurst::Glow {
            let value = format!(
               "{:.1}",
               ctx.appearance_settings.hold.burst_glow_from
            );
            let mut changed = false;
            Self::row(
               ui,
               theme,
               &q_mark_text,
               GLOW_FROM_TIP,
               "Glow radius",
               Some(&value),
               |ui| {
                  changed = ui
                     .add(
                        egui::Slider::new(
                           &mut ctx.appearance_settings.hold.burst_glow_from,
                           MIN_GLOW_FROM..=MAX_GLOW_FROM,
                        )
                        .step_by(0.1)
                        .show_value(false),
                     )
                     .changed();
               },
            );
            if changed {
               ctx.appearance_settings.clamp();
               self.dirty = true;
            }

            let value = format!(
               "{:.1}",
               ctx.appearance_settings.hold.burst_glow_to
            );
            let mut changed = false;
            Self::row(
               ui,
               theme,
               &q_mark_text,
               GLOW_TO_TIP,
               "Glow reach",
               Some(&value),
               |ui| {
                  changed = ui
                     .add(
                        egui::Slider::new(
                           &mut ctx.appearance_settings.hold.burst_glow_to,
                           MIN_GLOW_TO..=MAX_GLOW_TO,
                        )
                        .step_by(0.1)
                        .show_value(false),
                     )
                     .changed();
               },
            );
            if changed {
               ctx.appearance_settings.clamp();
               self.dirty = true;
            }

            let value = format!(
               "{:.2}",
               ctx.appearance_settings.hold.burst_glow_softness
            );
            let mut changed = false;
            Self::row(
               ui,
               theme,
               &q_mark_text,
               GLOW_SOFTNESS_TIP,
               "Softness",
               Some(&value),
               |ui| {
                  changed = ui
                     .add(
                        egui::Slider::new(
                           &mut ctx.appearance_settings.hold.burst_glow_softness,
                           MIN_GLOW_SOFTNESS..=MAX_GLOW_SOFTNESS,
                        )
                        .step_by(0.05)
                        .show_value(false),
                     )
                     .changed();
               },
            );
            if changed {
               ctx.appearance_settings.clamp();
               self.dirty = true;
            }
         }
      });

      ui.add_space(theme.spacing.xl);
      ui.label(RichText::new("Preview").size(theme.typography.very_large));

      let preview = Self::preview_settings(ctx, theme.colors.accent);
      let size = vec2(PREVIEW_BUTTON_W, PREVIEW_BUTTON_H);
      ui.vertical_centered(|ui| {
         let text = RichText::new("Hold to confirm").size(theme.typography.large);
         let button = Button::new(text)
            .visuals(button_visuals)
            .min_size(size)
            .label_align(Align::Center);

         if ui.add(button.hold_settings(&preview)).clicked() {
            self.preview_confirmed = true;
         }

         if self.preview_confirmed {
            let text = RichText::new("Confirmed").size(theme.typography.normal);
            ui.label(text.color(theme.colors.success));
         }

         let text = RichText::new("Reset").size(theme.typography.large);
         let button = Button::new(text)
            .visuals(button_visuals)
            .min_size(size)
            .label_align(Align::Center);

         if ui.add(button).clicked() {
            ctx.appearance_settings = AppearanceSettings::new();
            self.preview_confirmed = false;
            self.dirty = true;
         }
      });

      // One write per interaction: a slider drag releases the pointer before the
      // file is touched, and the value written is the clamped one the UI shows.
      if self.dirty && !ui.input(|i| i.pointer.any_down()) {
         ctx.appearance_settings.clamp();
         Self::persist(ctx);
         self.dirty = false;
      }
   }

   /// The preview always demonstrates the hold, even while the feature is off:
   /// it is the only way to feel the sweep and the burst without sending a tx.
   fn preview_settings(ctx: &ZeusContext, accent: Color32) -> HoldSettings {
      ctx.appearance_settings.resolved_hold(accent)
   }

   /// One setting row: label, control, value, then the question badge that
   /// explains it. Every column has a fixed width so the badge lands on the same
   /// x for sliders, the kind picker, the colour swatches and the checkboxes.
   fn row(
      ui: &mut Ui,
      theme: &Theme,
      q_mark_text: &RichText,
      tip: &'static str,
      label: &str,
      value: Option<&str>,
      add: impl FnOnce(&mut Ui),
   ) {
      let height = ui.spacing().interact_size.y;

      ui.horizontal(|ui| {
         Self::cell(ui, LABEL_W, height, |ui| {
            let text = RichText::new(label).size(theme.typography.normal);
            ui.add(egui::Label::new(text).extend());
         });

         Self::cell(ui, CONTROL_W, height, add);

         Self::cell(ui, VALUE_W, height, |ui| {
            if let Some(value) = value {
               let text = RichText::new(value).size(theme.typography.normal);
               ui.add(egui::Label::new(text).extend());
            }
         });

         let qmark = Badge::new(q_mark_text.clone(), BadgeTone::Info);
         ui.add(qmark).on_hover_text(tip);
      });
   }

   /// A fixed-width column inside [`Self::row`].
   fn cell(ui: &mut Ui, width: f32, height: f32, add: impl FnOnce(&mut Ui)) {
      ui.allocate_ui_with_layout(
         vec2(width, height),
         Layout::left_to_right(Align::Center),
         |ui| {
            ui.set_max_width(width);
            add(ui);
         },
      );
   }
}

fn srgb(color: Color32) -> [u8; 3] {
   let [r, g, b, _a] = color.to_srgba_unmultiplied();
   [r, g, b]
}
