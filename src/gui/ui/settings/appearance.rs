//! Settings → Appearance: hold-to-confirm and the completion burst.

use crate::core::ZeusContext;
use crate::core::types::{
   AmbientKind, AppearanceSettings, MAX_AMBIENT_BOLTS, MAX_AMBIENT_BRANCH_DEPTH,
   MAX_AMBIENT_DURATION_MS, MAX_AMBIENT_FLASH_INTENSITY, MAX_AMBIENT_INTERVAL_SECS,
   MAX_AMBIENT_JITTER, MAX_AMBIENT_LIGHT_DENSITY, MAX_AMBIENT_LIGHT_FLICKER,
   MAX_AMBIENT_LIGHT_INTENSITY, MAX_AMBIENT_LIGHT_RADIUS, MAX_AMBIENT_SNOW_DENSITY,
   MAX_AMBIENT_SNOW_SIZE, MAX_AMBIENT_SNOW_SPEED, MAX_AMBIENT_THICKNESS, MAX_AMBIENT_WIND_X,
   MAX_AMBIENT_WIND_Y, MAX_BRANCH_DEPTH, MAX_BURST_BOLTS, MAX_BURST_INTENSITY, MAX_BURST_MS,
   MAX_GLOW_FROM, MAX_GLOW_SOFTNESS, MAX_GLOW_TO, MAX_HOLD_SECONDS, MAX_JITTER, MAX_THICKNESS,
   MIN_AMBIENT_BOLTS, MIN_AMBIENT_BRANCH_DEPTH, MIN_AMBIENT_DURATION_MS,
   MIN_AMBIENT_FLASH_INTENSITY, MIN_AMBIENT_INTERVAL_SECS, MIN_AMBIENT_JITTER,
   MIN_AMBIENT_LIGHT_DENSITY, MIN_AMBIENT_LIGHT_FLICKER, MIN_AMBIENT_LIGHT_INTENSITY,
   MIN_AMBIENT_LIGHT_RADIUS, MIN_AMBIENT_SNOW_DENSITY, MIN_AMBIENT_SNOW_SIZE,
   MIN_AMBIENT_SNOW_SPEED, MIN_AMBIENT_THICKNESS, MIN_AMBIENT_WIND_X, MIN_AMBIENT_WIND_Y,
   MIN_BRANCH_DEPTH, MIN_BURST_BOLTS, MIN_BURST_INTENSITY, MIN_BURST_MS, MIN_GLOW_FROM,
   MIN_GLOW_SOFTNESS, MIN_GLOW_TO, MIN_HOLD_SECONDS, MIN_JITTER, MIN_THICKNESS,
   SECOND_COLOUR_ALPHA, accent_at,
};
use crate::utils::RT;
use egui::{Align, CollapsingHeader, Color32, Layout, RichText, Sense, Ui, UiBuilder, vec2};
use egui_elements::{Button, ComboBox, HoldBurst, HoldSettings, Label, Theme};
use elegance::{Badge, BadgeTone};

/// Fixed width of a row's label column, in points.
const LABEL_W: f32 = 150.0;
/// Fixed width of a row's control column, in points.
const CONTROL_W: f32 = 200.0;
/// Width of the kind combos, narrower than the control column so the box
/// does not fill the cell and crowd the value column.
const COMBO_W: f32 = 150.0;
/// Fixed width of a row's value cell, in points.
const VALUE_W: f32 = 64.0;
/// Width shared by the preview and Reset buttons, in points.
const PREVIEW_BUTTON_W: f32 = 170.0;
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
const AMBIENT_TIP: &str = "Paint ambient particles over the whole app. Snow and glow repaint \
                           continuously while they are on; lightning is episodic — a strike every \
                           interval, with nothing in between. The layer is Foreground, so the \
                           particles draw over the UI.";
const PARTICLE_TIP: &str = "Which ambient effect is painted. Exactly one kind is active at a \
                            time, and only that kind's settings are shown below.";
const SNOW_COLOUR_TIP: &str = "Colour of the snowflakes.";
const SNOW_DENSITY_TIP: &str = "How many snowflakes fall at once. The highest values cost the most \
                                frames.";
const SNOW_SIZE_TIP: &str = "Largest snowflake, in pixels.";
const SNOW_SPEED_TIP: &str = "Fastest fall speed, in pixels per second.";
const WIND_X_TIP: &str = "Horizontal wind. Positive pushes particles to the right.";
const WIND_Y_TIP: &str = "Vertical wind. Positive pushes particles down.";
const GLOW_COLOUR_TIP: &str = "Colour of the glow motes.";
const GLOW_DENSITY_TIP: &str = "How many motes drift at once.";
const GLOW_RADIUS_TIP: &str = "Largest mote radius, in pixels.";
const GLOW_INTENSITY_TIP: &str = "Strength of the glow. 1 is the effect's own profile.";
const GLOW_FLICKER_TIP: &str = "How much each mote's brightness flickers. 0 is steady.";
const LIGHTNING_COLOUR_TIP: &str = "Colour of the bolt and its flash.";
const LIGHTNING_BOLTS_TIP: &str = "How many bolts strike at once.";
const LIGHTNING_DEPTH_TIP: &str = "How many times each bolt branches. 0 draws the trunk only.";
const LIGHTNING_THICKNESS_TIP: &str = "Thickness of the bolt's core stroke, in pixels.";
const LIGHTNING_JITTER_TIP: &str = "How far the bolt wanders between segments, as a fraction of \
                                    each segment's length.";
const LIGHTNING_FLASH_TIP: &str = "Peak strength of the full-area flash.";
const LIGHTNING_DURATION_TIP: &str = "How long a strike lasts, in milliseconds.";
const LIGHTNING_INTERVAL_TIP: &str = "Seconds between ambient strikes. 0 disables the ambient \
                                      scheduler.";
const SNOW_MORE_TIP: &str = "Open the crate's full snow settings, including minimum size, minimum \
                             speed and layer.";
const GLOW_MORE_TIP: &str = "Open the crate's full glow settings, including motion, minimum \
                             radius, lifetimes and layer.";
const LIGHTNING_MORE_TIP: &str = "Open the crate's full lightning settings.";

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
      let settings = ctx.appearance_settings.clone();
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

      if Self::section(ui, theme, "Hold to confirm", |ui| {
         Self::hold_block(ctx, theme, &q_mark_text, ui)
      }) {
         self.dirty = true;
      }

      if Self::section(ui, theme, "Ambient", |ui| {
         Self::ambient_block(ctx, theme, &q_mark_text, ui)
      }) {
         self.dirty = true;
      }

      ui.add_space(theme.spacing.xl);
      ui.vertical_centered(|ui| {
         ui.label(RichText::new("Preview").size(theme.typography.very_large));
      });

      let preview = Self::preview_settings(ctx, theme.colors.accent);
      let size = vec2(PREVIEW_BUTTON_W, PREVIEW_BUTTON_H);
      ui.vertical_centered(|ui| {
         ui.horizontal(|ui| {
            // One sm step between the pair, not the page's tight xs; the pad
            // below includes it so the pair stays centred as a unit.
            ui.spacing_mut().item_spacing.x = theme.spacing.sm;
            let pair_width = PREVIEW_BUTTON_W * 2.0 + theme.spacing.sm;
            let pad = ((ui.available_width() - pair_width) / 2.0).max(0.0);
            ui.add_space(pad);

            let text = RichText::new("Hold to confirm").size(theme.typography.large);
            let button = Button::new(text)
               .visuals(button_visuals)
               .min_size(size)
               .label_align(Align::Center);

            if ui.add(button.hold_settings(&preview)).clicked() {
               self.preview_confirmed = true;
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

         if self.preview_confirmed {
            let text = RichText::new("Confirmed").size(theme.typography.normal);
            ui.label(text.color(theme.colors.success));
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

   /// One collapsible framed section. Closed by default: the point is a clean
   /// page, and egui remembers each header's state by id.
   fn section(ui: &mut Ui, theme: &Theme, title: &str, body: impl FnOnce(&mut Ui) -> bool) -> bool {
      let frame = theme.frame2;
      frame
         .show(ui, |ui| {
            ui.set_width(ui.available_width());
            CollapsingHeader::new(RichText::new(title).size(theme.typography.very_large))
               .id_salt(title)
               .show(ui, |ui| body(ui))
               .body_returned
               .unwrap_or(false)
         })
         .inner
   }

   /// The hold-to-confirm master switch and every hold knob.
   ///
   /// Returns whether a persisted value changed.
   fn hold_block(
      ctx: &mut ZeusContext,
      theme: &Theme,
      q_mark_text: &RichText,
      ui: &mut Ui,
   ) -> bool {
      let combo_visuals = theme.combo_box_visuals();
      let label_visuals = theme.label_visuals();

      let mut enabled = ctx.appearance_settings.hold_to_confirm();
      let mut dirty = false;
      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         ENABLE_TIP,
         "Hold to confirm",
         None,
         None,
         |ui| {
            changed = ui.checkbox(&mut enabled, "").changed();
         },
      );
      if changed {
         ctx.appearance_settings.set_hold_to_confirm(enabled);
         dirty = true;
      }

      ui.add_enabled_ui(enabled, |ui| {
         let mut seconds = ctx.appearance_settings.hold_seconds();
         let value = format!("{seconds:.2} s");
         let mut changed = false;
         Self::row(
            ui,
            theme,
            q_mark_text,
            DURATION_TIP,
            "Duration",
            Some(&value),
            None,
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
            dirty = true;
         }

         ui.add_space(theme.spacing.xl);
         ui.label(RichText::new("Fill").size(theme.typography.large));

         let mut use_accent = ctx.appearance_settings.hold.fill_from.is_none();
         let mut changed = false;
         Self::row(
            ui,
            theme,
            q_mark_text,
            ACCENT_TIP,
            "Theme accent",
            None,
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
            dirty = true;
         }

         if !use_accent {
            let mut rgb =
               srgb(ctx.appearance_settings.hold.fill_from.unwrap_or(theme.colors.accent));
            let mut changed = false;
            Self::row(
               ui,
               theme,
               q_mark_text,
               FILL_FROM_TIP,
               "Start colour",
               None,
               None,
               |ui| {
                  changed = ui.color_edit_button_srgb(&mut rgb).changed();
               },
            );
            if changed {
               ctx.appearance_settings.hold.fill_from =
                  Some(Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
               dirty = true;
            }
         }

         let mut shift = ctx.appearance_settings.shift_second_colour;
         let mut changed = false;
         Self::row(
            ui,
            theme,
            q_mark_text,
            SHIFT_TIP,
            "Second colour",
            None,
            None,
            |ui| {
               changed = ui.checkbox(&mut shift, "").changed();
            },
         );
         if changed {
            ctx.appearance_settings.shift_second_colour = shift;
            dirty = true;
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
               q_mark_text,
               END_COLOUR_TIP,
               "End colour",
               None,
               None,
               |ui| {
                  changed = ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed();
               },
            );
            if changed {
               ctx.appearance_settings.second_colour = Some(Color32::from_rgba_unmultiplied(
                  rgba[0], rgba[1], rgba[2], rgba[3],
               ));
               dirty = true;
            }
         }

         ui.add_space(theme.spacing.xl);
         ui.label(RichText::new("Burst").size(theme.typography.large));

         let mut chosen = ctx.appearance_settings.hold.burst;
         let selected_text = RichText::new(chosen.to_str()).size(theme.typography.normal);
         let kind_label = Label::new(selected_text, None)
            .visuals(label_visuals)
            .sense(Sense::click())
            .expand(Some(6.0))
            .interactive(true)
            .fill_width(true);
         let mut changed = false;
         let combo_h = Self::combo_row_h(theme, ui);
         Self::row(
            ui,
            theme,
            q_mark_text,
            BURST_TIP,
            "Kind",
            None,
            Some(combo_h),
            |ui| {
               ComboBox::new("appearance_burst_kind", kind_label)
                  .width(COMBO_W)
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
            dirty = true;
         }

         let mut same_color = ctx.appearance_settings.hold.burst_color.is_none();
         let mut changed = false;
         Self::row(
            ui,
            theme,
            q_mark_text,
            BURST_COLOR_TIP,
            "Same as fill",
            None,
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
            dirty = true;
         }

         if !same_color {
            let mut rgb =
               srgb(ctx.appearance_settings.hold.burst_color.unwrap_or(theme.colors.accent));
            let mut changed = false;
            Self::row(
               ui,
               theme,
               q_mark_text,
               BURST_COLOR_TIP,
               "Burst colour",
               None,
               None,
               |ui| {
                  changed = ui.color_edit_button_srgb(&mut rgb).changed();
               },
            );
            if changed {
               ctx.appearance_settings.hold.burst_color =
                  Some(Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
               dirty = true;
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
            q_mark_text,
            INTENSITY_TIP,
            "Intensity",
            Some(&value),
            None,
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
            dirty = true;
         }

         let value = format!("{:.0} ms", ctx.appearance_settings.hold.burst_ms);
         let mut changed = false;
         Self::row(
            ui,
            theme,
            q_mark_text,
            BURST_MS_TIP,
            "Burst length",
            Some(&value),
            None,
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
            dirty = true;
         }

         if ctx.appearance_settings.hold.burst == HoldBurst::Lightning {
            let value = format!("{}", ctx.appearance_settings.hold.burst_bolts);
            let mut changed = false;
            Self::row(
               ui,
               theme,
               q_mark_text,
               BOLTS_TIP,
               "Bolts",
               Some(&value),
               None,
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
               dirty = true;
            }

            let value = format!(
               "{}",
               ctx.appearance_settings.hold.burst_branch_depth
            );
            let mut changed = false;
            Self::row(
               ui,
               theme,
               q_mark_text,
               DEPTH_TIP,
               "Branch depth",
               Some(&value),
               None,
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
               dirty = true;
            }

            let value = format!(
               "{:.1}",
               ctx.appearance_settings.hold.burst_thickness
            );
            let mut changed = false;
            Self::row(
               ui,
               theme,
               q_mark_text,
               THICKNESS_TIP,
               "Thickness",
               Some(&value),
               None,
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
               dirty = true;
            }

            let value = format!("{:.3}", ctx.appearance_settings.hold.burst_jitter);
            let mut changed = false;
            Self::row(
               ui,
               theme,
               q_mark_text,
               JITTER_TIP,
               "Jitter",
               Some(&value),
               None,
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
               dirty = true;
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
               q_mark_text,
               GLOW_FROM_TIP,
               "Glow radius",
               Some(&value),
               None,
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
               dirty = true;
            }

            let value = format!(
               "{:.1}",
               ctx.appearance_settings.hold.burst_glow_to
            );
            let mut changed = false;
            Self::row(
               ui,
               theme,
               q_mark_text,
               GLOW_TO_TIP,
               "Glow reach",
               Some(&value),
               None,
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
               dirty = true;
            }

            let value = format!(
               "{:.2}",
               ctx.appearance_settings.hold.burst_glow_softness
            );
            let mut changed = false;
            Self::row(
               ui,
               theme,
               q_mark_text,
               GLOW_SOFTNESS_TIP,
               "Softness",
               Some(&value),
               None,
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
               dirty = true;
            }
         }
      });
      dirty
   }

   /// The preview always demonstrates the hold, even while the feature is off:
   /// it is the only way to feel the sweep and the burst without sending a tx.
   fn preview_settings(ctx: &ZeusContext, accent: Color32) -> HoldSettings {
      ctx.appearance_settings.resolved_hold(accent)
   }

   /// Master switch, the one ambient-kind combo, and the chosen kind's knobs.
   ///
   /// Returns whether a persisted value changed.
   fn ambient_block(
      ctx: &mut ZeusContext,
      theme: &Theme,
      q_mark_text: &RichText,
      ui: &mut Ui,
   ) -> bool {
      let combo_visuals = theme.combo_box_visuals();
      let label_visuals = theme.label_visuals();

      let mut dirty = false;
      let mut enabled = ctx.appearance_settings.ambient_enabled;
      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         AMBIENT_TIP,
         "Ambient particles",
         None,
         None,
         |ui| {
            changed = ui.checkbox(&mut enabled, "").changed();
         },
      );
      if changed {
         ctx.appearance_settings.ambient_enabled = enabled;
         if !enabled {
            // The per-kind block below is disabled with the master switch;
            // close any crate window that was open rather than leave it
            // interactive outside the disabled pass.
            ctx.appearance_settings.snow.open = false;
            ctx.appearance_settings.light.open = false;
            ctx.appearance_settings.lightning.open = false;
         }
         dirty = true;
      }

      ui.add_enabled_ui(enabled, |ui| {
         let mut kind = ctx.appearance_settings.ambient_kind;
         let selected_text = RichText::new(kind.label()).size(theme.typography.normal);
         let kind_label = Label::new(selected_text, None)
            .visuals(label_visuals)
            .sense(Sense::click())
            .expand(Some(6.0))
            .interactive(true)
            .fill_width(true);
         let current = kind;
         let mut changed = false;
         let combo_h = Self::combo_row_h(theme, ui);
         Self::row(
            ui,
            theme,
            q_mark_text,
            PARTICLE_TIP,
            "Particle",
            None,
            Some(combo_h),
            |ui| {
               ComboBox::new("appearance_ambient_kind", kind_label)
                  .width(COMBO_W)
                  .visuals(combo_visuals)
                  .show_ui(ui, |ui| {
                     ui.spacing_mut().item_spacing.y = theme.spacing.sm;

                     for option in AmbientKind::ALL {
                        let text = RichText::new(option.label()).size(theme.typography.normal);
                        let label = Label::new(text, None)
                           .visuals(label_visuals)
                           .expand(Some(6.0))
                           .sense(Sense::click())
                           .interactive(true)
                           .fill_width(true);

                        if ui.add(label).clicked() {
                           kind = option;
                        }
                     }
                  });
               changed = kind != current;
            },
         );
         if changed {
            ctx.appearance_settings.ambient_kind = kind;
            ctx.appearance_settings.clamp();
            dirty = true;
         }

         dirty |= match ctx.appearance_settings.ambient_kind {
            AmbientKind::Snow => Self::snow_block(ctx, theme, q_mark_text, ui),
            AmbientKind::Glow => Self::glow_block(ctx, theme, q_mark_text, ui),
            AmbientKind::Lightning => Self::lightning_block(ctx, theme, q_mark_text, ui),
         };
      });

      dirty
   }

   fn snow_block(
      ctx: &mut ZeusContext,
      theme: &Theme,
      q_mark_text: &RichText,
      ui: &mut Ui,
   ) -> bool {
      let mut dirty = false;
      let button_visuals = theme.button_visuals();

      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         SNOW_COLOUR_TIP,
         "Colour",
         None,
         None,
         |ui| {
            changed = ui.color_edit_button_srgba(&mut ctx.appearance_settings.snow.color).changed();
         },
      );
      if changed {
         dirty = true;
      }

      let value = format!("{}", ctx.appearance_settings.snow.density);
      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         SNOW_DENSITY_TIP,
         "Density",
         Some(&value),
         None,
         |ui| {
            changed = ui
               .add(
                  egui::Slider::new(
                     &mut ctx.appearance_settings.snow.density,
                     MIN_AMBIENT_SNOW_DENSITY..=MAX_AMBIENT_SNOW_DENSITY,
                  )
                  .logarithmic(true)
                  .show_value(false),
               )
               .changed();
         },
      );
      if changed {
         ctx.appearance_settings.clamp();
         dirty = true;
      }

      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         SNOW_SIZE_TIP,
         "Size max",
         None,
         None,
         |ui| {
            changed = Self::drag_value(
               ui,
               egui::DragValue::new(&mut ctx.appearance_settings.snow.size_max)
                  .speed(0.1)
                  .range(MIN_AMBIENT_SNOW_SIZE..=MAX_AMBIENT_SNOW_SIZE),
            )
            .changed();
         },
      );
      if changed {
         ctx.appearance_settings.clamp();
         dirty = true;
      }

      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         SNOW_SPEED_TIP,
         "Speed max",
         None,
         None,
         |ui| {
            changed = Self::drag_value(
               ui,
               egui::DragValue::new(&mut ctx.appearance_settings.snow.speed_max)
                  .speed(1.0)
                  .range(MIN_AMBIENT_SNOW_SPEED..=MAX_AMBIENT_SNOW_SPEED),
            )
            .changed();
         },
      );
      if changed {
         ctx.appearance_settings.clamp();
         dirty = true;
      }

      let value = format!("{:.0}", ctx.appearance_settings.snow.wind.x);
      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         WIND_X_TIP,
         "Wind X",
         Some(&value),
         None,
         |ui| {
            changed = ui
               .add(
                  egui::Slider::new(
                     &mut ctx.appearance_settings.snow.wind.x,
                     MIN_AMBIENT_WIND_X..=MAX_AMBIENT_WIND_X,
                  )
                  .show_value(false),
               )
               .changed();
         },
      );
      if changed {
         ctx.appearance_settings.clamp();
         dirty = true;
      }

      let value = format!("{:.0}", ctx.appearance_settings.snow.wind.y);
      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         WIND_Y_TIP,
         "Wind Y",
         Some(&value),
         None,
         |ui| {
            changed = ui
               .add(
                  egui::Slider::new(
                     &mut ctx.appearance_settings.snow.wind.y,
                     MIN_AMBIENT_WIND_Y..=MAX_AMBIENT_WIND_Y,
                  )
                  .show_value(false),
               )
               .changed();
         },
      );
      if changed {
         ctx.appearance_settings.clamp();
         dirty = true;
      }

      let mut more = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         SNOW_MORE_TIP,
         "",
         None,
         None,
         |ui| {
            let text = RichText::new("More…").size(theme.typography.normal);
            more = ui.add(Button::new(text).visuals(button_visuals)).clicked();
         },
      );
      if more {
         ctx.appearance_settings.snow.open = true;
      }
      if ctx.appearance_settings.snow.open {
         ctx.appearance_settings.snow.show_window(ui.ctx());
      }

      ui.add_space(theme.spacing.md);
      dirty
   }

   fn glow_block(
      ctx: &mut ZeusContext,
      theme: &Theme,
      q_mark_text: &RichText,
      ui: &mut Ui,
   ) -> bool {
      let mut dirty = false;
      let button_visuals = theme.button_visuals();

      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         GLOW_COLOUR_TIP,
         "Colour",
         None,
         None,
         |ui| {
            changed =
               ui.color_edit_button_srgba(&mut ctx.appearance_settings.light.color).changed();
         },
      );
      if changed {
         dirty = true;
      }

      let value = format!("{}", ctx.appearance_settings.light.density);
      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         GLOW_DENSITY_TIP,
         "Density",
         Some(&value),
         None,
         |ui| {
            changed = ui
               .add(
                  egui::Slider::new(
                     &mut ctx.appearance_settings.light.density,
                     MIN_AMBIENT_LIGHT_DENSITY..=MAX_AMBIENT_LIGHT_DENSITY,
                  )
                  .logarithmic(true)
                  .show_value(false),
               )
               .changed();
         },
      );
      if changed {
         ctx.appearance_settings.clamp();
         dirty = true;
      }

      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         GLOW_RADIUS_TIP,
         "Radius max",
         None,
         None,
         |ui| {
            changed = Self::drag_value(
               ui,
               egui::DragValue::new(&mut ctx.appearance_settings.light.radius_max)
                  .speed(0.5)
                  .range(MIN_AMBIENT_LIGHT_RADIUS..=MAX_AMBIENT_LIGHT_RADIUS),
            )
            .changed();
         },
      );
      if changed {
         ctx.appearance_settings.clamp();
         dirty = true;
      }

      let value = format!("{:.2}", ctx.appearance_settings.light.intensity);
      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         GLOW_INTENSITY_TIP,
         "Intensity",
         Some(&value),
         None,
         |ui| {
            changed = ui
               .add(
                  egui::Slider::new(
                     &mut ctx.appearance_settings.light.intensity,
                     MIN_AMBIENT_LIGHT_INTENSITY..=MAX_AMBIENT_LIGHT_INTENSITY,
                  )
                  .step_by(0.05)
                  .show_value(false),
               )
               .changed();
         },
      );
      if changed {
         ctx.appearance_settings.clamp();
         dirty = true;
      }

      let value = format!("{:.2}", ctx.appearance_settings.light.flicker);
      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         GLOW_FLICKER_TIP,
         "Flicker",
         Some(&value),
         None,
         |ui| {
            changed = ui
               .add(
                  egui::Slider::new(
                     &mut ctx.appearance_settings.light.flicker,
                     MIN_AMBIENT_LIGHT_FLICKER..=MAX_AMBIENT_LIGHT_FLICKER,
                  )
                  .step_by(0.05)
                  .show_value(false),
               )
               .changed();
         },
      );
      if changed {
         ctx.appearance_settings.clamp();
         dirty = true;
      }

      let mut more = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         GLOW_MORE_TIP,
         "",
         None,
         None,
         |ui| {
            let text = RichText::new("More…").size(theme.typography.normal);
            more = ui.add(Button::new(text).visuals(button_visuals)).clicked();
         },
      );
      if more {
         ctx.appearance_settings.light.open = true;
      }
      if ctx.appearance_settings.light.open {
         ctx.appearance_settings.light.show_window(ui.ctx());
      }

      ui.add_space(theme.spacing.md);
      dirty
   }

   fn lightning_block(
      ctx: &mut ZeusContext,
      theme: &Theme,
      q_mark_text: &RichText,
      ui: &mut Ui,
   ) -> bool {
      let mut dirty = false;
      let button_visuals = theme.button_visuals();

      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         LIGHTNING_COLOUR_TIP,
         "Colour",
         None,
         None,
         |ui| {
            changed = ui
               .color_edit_button_srgba(&mut ctx.appearance_settings.lightning.color)
               .changed();
         },
      );
      if changed {
         dirty = true;
      }

      let value = format!("{}", ctx.appearance_settings.lightning.bolt_count);
      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         LIGHTNING_BOLTS_TIP,
         "Bolts",
         Some(&value),
         None,
         |ui| {
            changed = ui
               .add(
                  egui::Slider::new(
                     &mut ctx.appearance_settings.lightning.bolt_count,
                     MIN_AMBIENT_BOLTS..=MAX_AMBIENT_BOLTS,
                  )
                  .show_value(false),
               )
               .changed();
         },
      );
      if changed {
         ctx.appearance_settings.clamp();
         dirty = true;
      }

      let value = format!(
         "{}",
         ctx.appearance_settings.lightning.branch_depth
      );
      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         LIGHTNING_DEPTH_TIP,
         "Branch depth",
         Some(&value),
         None,
         |ui| {
            changed = ui
               .add(
                  egui::Slider::new(
                     &mut ctx.appearance_settings.lightning.branch_depth,
                     MIN_AMBIENT_BRANCH_DEPTH..=MAX_AMBIENT_BRANCH_DEPTH,
                  )
                  .show_value(false),
               )
               .changed();
         },
      );
      if changed {
         ctx.appearance_settings.clamp();
         dirty = true;
      }

      let value = format!(
         "{:.1}",
         ctx.appearance_settings.lightning.thickness
      );
      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         LIGHTNING_THICKNESS_TIP,
         "Thickness",
         Some(&value),
         None,
         |ui| {
            changed = ui
               .add(
                  egui::Slider::new(
                     &mut ctx.appearance_settings.lightning.thickness,
                     MIN_AMBIENT_THICKNESS..=MAX_AMBIENT_THICKNESS,
                  )
                  .step_by(0.1)
                  .show_value(false),
               )
               .changed();
         },
      );
      if changed {
         ctx.appearance_settings.clamp();
         dirty = true;
      }

      let value = format!("{:.2}", ctx.appearance_settings.lightning.jitter);
      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         LIGHTNING_JITTER_TIP,
         "Jitter",
         Some(&value),
         None,
         |ui| {
            changed = ui
               .add(
                  egui::Slider::new(
                     &mut ctx.appearance_settings.lightning.jitter,
                     MIN_AMBIENT_JITTER..=MAX_AMBIENT_JITTER,
                  )
                  .step_by(0.01)
                  .show_value(false),
               )
               .changed();
         },
      );
      if changed {
         ctx.appearance_settings.clamp();
         dirty = true;
      }

      let value = format!(
         "{:.2}",
         ctx.appearance_settings.lightning.flash_intensity
      );
      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         LIGHTNING_FLASH_TIP,
         "Flash intensity",
         Some(&value),
         None,
         |ui| {
            changed = ui
               .add(
                  egui::Slider::new(
                     &mut ctx.appearance_settings.lightning.flash_intensity,
                     MIN_AMBIENT_FLASH_INTENSITY..=MAX_AMBIENT_FLASH_INTENSITY,
                  )
                  .step_by(0.05)
                  .show_value(false),
               )
               .changed();
         },
      );
      if changed {
         ctx.appearance_settings.clamp();
         dirty = true;
      }

      let value = format!(
         "{:.0} ms",
         ctx.appearance_settings.lightning.duration_ms
      );
      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         LIGHTNING_DURATION_TIP,
         "Duration",
         Some(&value),
         None,
         |ui| {
            changed = ui
               .add(
                  egui::Slider::new(
                     &mut ctx.appearance_settings.lightning.duration_ms,
                     MIN_AMBIENT_DURATION_MS..=MAX_AMBIENT_DURATION_MS,
                  )
                  .step_by(10.0)
                  .show_value(false),
               )
               .changed();
         },
      );
      if changed {
         ctx.appearance_settings.clamp();
         dirty = true;
      }

      let mut changed = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         LIGHTNING_INTERVAL_TIP,
         "Interval",
         None,
         None,
         |ui| {
            changed = Self::drag_value(
               ui,
               egui::DragValue::new(&mut ctx.appearance_settings.lightning.ambient_interval_secs)
                  .speed(0.1)
                  .range(MIN_AMBIENT_INTERVAL_SECS..=MAX_AMBIENT_INTERVAL_SECS)
                  .suffix(" s"),
            )
            .changed();
         },
      );
      if changed {
         ctx.appearance_settings.clamp();
         dirty = true;
      }

      let mut more = false;
      Self::row(
         ui,
         theme,
         q_mark_text,
         LIGHTNING_MORE_TIP,
         "",
         None,
         None,
         |ui| {
            let text = RichText::new("More…").size(theme.typography.normal);
            more = ui.add(Button::new(text).visuals(button_visuals)).clicked();
         },
      );
      if more {
         ctx.appearance_settings.lightning.open = true;
      }
      if ctx.appearance_settings.lightning.open {
         ctx.appearance_settings.lightning.show_window(ui.ctx());
      }

      ui.add_space(theme.spacing.md);
      dirty
   }

   /// Height the crate's `ComboBox` paints for its selected item: one line of
   /// the theme's normal text (or the disclosure icon, whichever is taller)
   /// plus `theme.button_padding` on both sides — the same inputs its button
   /// size is built from, so a combo row reserves exactly what it draws.
   fn combo_row_h(theme: &Theme, ui: &Ui) -> f32 {
      let font = egui::FontId::proportional(theme.typography.normal);
      let line = ui.fonts_mut(|fonts| fonts.row_height(&font));
      line.max(ui.spacing().icon_width).round() + 2.0 * theme.button_padding.y
   }

   /// One setting row: label, control, value, then the question badge that
   /// explains it. Every column has a fixed width so the badge lands on the same
   /// x for sliders, the kind picker, the colour swatches and the checkboxes.
   /// `height` overrides the cell height; combo rows pass [`Self::combo_row_h`]
   /// because the crate's `ComboBox` paints a taller button than
   /// `interact_size.y` and would otherwise overflow into the next row's gap.
   fn row(
      ui: &mut Ui,
      theme: &Theme,
      q_mark_text: &RichText,
      tip: &'static str,
      label: &str,
      value: Option<&str>,
      height: Option<f32>,
      add: impl FnOnce(&mut Ui),
   ) {
      let height = height.unwrap_or_else(|| ui.spacing().interact_size.y);

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

   /// A `DragValue` in a row's control cell, sized to the value column plus its
   /// own chrome. The value cell stays empty so the number is printed once.
   fn drag_value(ui: &mut Ui, value: egui::DragValue<'_>) -> egui::Response {
      let height = ui.spacing().interact_size.y;
      ui.add_sized(vec2(VALUE_W + 16.0, height), value)
   }

   /// A fixed-width column inside [`Self::row`].
   ///
   /// The cell reserves its width exactly. `allocate_ui_with_layout` used to
   /// shrink the child to its content, so every column after it slid by the
   /// label's width and the question badges landed at a different x per row.
   fn cell(ui: &mut Ui, width: f32, height: f32, add: impl FnOnce(&mut Ui)) {
      let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
      let mut child =
         ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
      add(&mut child);
   }
}

fn srgb(color: Color32) -> [u8; 3] {
   let [r, g, b, _a] = color.to_srgba_unmultiplied();
   [r, g, b]
}

#[cfg(test)]
mod tests {
   use super::*;

   /// Mike's alignment bug: the old cell child shrank to its content, so a
   /// 150 pt cell only moved the cursor as far as its label ended and every
   /// column after it slid per row. A cell must reserve its full width plus one
   /// item spacing for the next widget.
   #[test]
   fn a_cell_reserves_its_width_for_the_next_widget() {
      egui::__run_test_ui(|ui| {
         ui.spacing_mut().item_spacing.x = 8.0;
         ui.horizontal(|ui| {
            let start = ui.cursor().left();
            let spacing = ui.spacing().item_spacing.x;

            let mut widget_left = None;
            AppearanceSettingsUi::cell(ui, 150.0, 20.0, |ui| {
               let (rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
               widget_left = Some(rect.left());
            });

            let (next, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());

            assert_eq!(widget_left, Some(start));
            assert_eq!(
               next.left(),
               start + 150.0 + spacing,
               "the cell must reserve its full width, not shrink to its content"
            );
         });
      });
   }

   /// Every row is three `cell`s plus the badge, so the control column, the
   /// value column and the badge share one x across rows regardless of how wide
   /// a label's text is.
   #[test]
   fn the_row_columns_and_badge_share_one_x() {
      egui::__run_test_ui(|ui| {
         ui.spacing_mut().item_spacing.x = 8.0;
         ui.horizontal(|ui| {
            let start = ui.cursor().left();
            let spacing = ui.spacing().item_spacing.x;

            let mut control_left = None;
            let mut value_left = None;
            AppearanceSettingsUi::cell(ui, LABEL_W, 20.0, |_| {});
            AppearanceSettingsUi::cell(ui, CONTROL_W, 20.0, |ui| {
               control_left = Some(ui.cursor().left());
            });
            AppearanceSettingsUi::cell(ui, VALUE_W, 20.0, |ui| {
               value_left = Some(ui.cursor().left());
            });
            let (badge, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());

            let control_x = start + LABEL_W + spacing;
            let value_x = control_x + CONTROL_W + spacing;
            assert_eq!(control_left, Some(control_x));
            assert_eq!(value_left, Some(value_x));
            assert_eq!(badge.left(), value_x + VALUE_W + spacing);
         });
      });
   }
}
