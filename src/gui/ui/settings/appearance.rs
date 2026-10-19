//! Settings → Appearance: hold-to-confirm and the completion burst.

use crate::core::ZeusContext;
use crate::core::types::{
   AppearanceSettings, MAX_BRANCH_DEPTH, MAX_BURST_BOLTS, MAX_BURST_INTENSITY, MAX_BURST_MS,
   MAX_GLOW_FROM, MAX_GLOW_SOFTNESS, MAX_GLOW_TO, MAX_HOLD_SECONDS, MAX_JITTER, MAX_THICKNESS,
   MIN_BRANCH_DEPTH, MIN_BURST_BOLTS, MIN_BURST_INTENSITY, MIN_BURST_MS, MIN_GLOW_FROM,
   MIN_GLOW_SOFTNESS, MIN_GLOW_TO, MIN_HOLD_SECONDS, MIN_JITTER, MIN_THICKNESS,
};
use crate::utils::RT;
use egui::{Align, Color32, Layout, RichText, Sense, Ui, Vec2, vec2};
use egui_elements::{Button, ComboBox, HoldBurst, HoldSettings, Label, Theme};
use elegance::{Badge, BadgeTone};

const ENABLE_TIP: &str = "Require a hold on Confirm and Sign. Reject and Cancel stay instant. \
                          Off by default, and the sweep plus the burst only appear while it is on.";
const DURATION_TIP: &str = "How long the affirmative button must be held before it fires. \
                            A misclick is released long before this.";
const ACCENT_TIP: &str = "Sweep with the theme's accent colour instead of a colour of your own.";
const FILL_FROM_TIP: &str = "Colour the hold's sweep starts at.";
const FILL_TO_TIP: &str = "Second colour the sweep shifts to as the hold completes. \
                           Leave it off to keep the sweep one solid colour.";
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
      let row_size = vec2(ui.available_width(), ui.spacing().interact_size.y);
      let q_mark_text = RichText::new("?").size(theme.typography.normal);
      let combo_visuals = theme.combo_box_visuals();
      let label_visuals = theme.label_visuals();

      ui.label(RichText::new("Hold to Confirm").size(theme.typography.very_large));

      let mut enabled = ctx.appearance_settings.hold_to_confirm();
      let mut changed = false;
      Self::row(ui, row_size, &q_mark_text, ENABLE_TIP, |ui| {
         let text = RichText::new("Require a hold to confirm").size(theme.typography.normal);
         changed = ui.checkbox(&mut enabled, text).changed();
      });
      if changed {
         ctx.appearance_settings.set_hold_to_confirm(enabled);
         self.dirty = true;
      }

      ui.add_enabled_ui(enabled, |ui| {
         let mut seconds = ctx.appearance_settings.hold_seconds();
         let mut changed = false;
         Self::row(ui, row_size, &q_mark_text, DURATION_TIP, |ui| {
            ui.label(RichText::new("Duration").size(theme.typography.normal));
            changed = ui
               .add(
                  egui::Slider::new(&mut seconds, MIN_HOLD_SECONDS..=MAX_HOLD_SECONDS)
                     .step_by(0.05)
                     .suffix(" s"),
               )
               .changed();
         });
         if changed {
            ctx.appearance_settings.set_hold_seconds(seconds);
            self.dirty = true;
         }

         ui.add_space(theme.spacing.xl);
         ui.label(RichText::new("Fill").size(theme.typography.very_large));

         let mut use_accent = ctx.appearance_settings.hold.fill_from.is_none();
         let mut changed = false;
         Self::row(ui, row_size, &q_mark_text, ACCENT_TIP, |ui| {
            let text = RichText::new("Use the theme accent").size(theme.typography.normal);
            changed = ui.checkbox(&mut use_accent, text).changed();
         });
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
            Self::row(ui, row_size, &q_mark_text, FILL_FROM_TIP, |ui| {
               ui.label(RichText::new("Start colour").size(theme.typography.normal));
               changed = ui.color_edit_button_srgb(&mut rgb).changed();
            });
            if changed {
               ctx.appearance_settings.hold.fill_from =
                  Some(Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
               self.dirty = true;
            }
         }

         let mut shift = ctx.appearance_settings.hold.fill_to.is_some();
         let mut changed = false;
         Self::row(ui, row_size, &q_mark_text, FILL_TO_TIP, |ui| {
            let text = RichText::new("Shift to a second colour").size(theme.typography.normal);
            changed = ui.checkbox(&mut shift, text).changed();
         });
         if changed {
            ctx.appearance_settings.hold.fill_to = if shift {
               Some(ctx.appearance_settings.hold.fill_from.unwrap_or(theme.colors.accent))
            } else {
               None
            };
            self.dirty = true;
         }

         if shift {
            let mut rgb = srgb(ctx.appearance_settings.hold.fill_to.unwrap_or(theme.colors.accent));
            let mut changed = false;
            Self::row(ui, row_size, &q_mark_text, FILL_TO_TIP, |ui| {
               ui.label(RichText::new("End colour").size(theme.typography.normal));
               changed = ui.color_edit_button_srgb(&mut rgb).changed();
            });
            if changed {
               ctx.appearance_settings.hold.fill_to =
                  Some(Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
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
         Self::row(ui, row_size, &q_mark_text, BURST_TIP, |ui| {
            ui.label(RichText::new("Kind").size(theme.typography.normal));
            ComboBox::new("appearance_burst_kind", kind_label)
               .width(ui.available_width() * 0.4)
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
         });
         if changed {
            ctx.appearance_settings.hold.burst = chosen;
            self.dirty = true;
         }

         let mut same_color = ctx.appearance_settings.hold.burst_color.is_none();
         let mut changed = false;
         Self::row(
            ui,
            row_size,
            &q_mark_text,
            BURST_COLOR_TIP,
            |ui| {
               let text = RichText::new("Same colour as the fill").size(theme.typography.normal);
               changed = ui.checkbox(&mut same_color, text).changed();
            },
         );
         if changed {
            ctx.appearance_settings.hold.burst_color = if same_color {
               None
            } else {
               Some(
                  ctx.appearance_settings
                     .hold
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
               row_size,
               &q_mark_text,
               BURST_COLOR_TIP,
               |ui| {
                  ui.label(RichText::new("Burst colour").size(theme.typography.normal));
                  changed = ui.color_edit_button_srgb(&mut rgb).changed();
               },
            );
            if changed {
               ctx.appearance_settings.hold.burst_color =
                  Some(Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
               self.dirty = true;
            }
         }

         let mut changed = false;
         Self::row(ui, row_size, &q_mark_text, INTENSITY_TIP, |ui| {
            ui.label(RichText::new("Intensity").size(theme.typography.normal));
            changed = ui
               .add(
                  egui::Slider::new(
                     &mut ctx.appearance_settings.hold.burst_intensity,
                     MIN_BURST_INTENSITY..=MAX_BURST_INTENSITY,
                  )
                  .step_by(0.05),
               )
               .changed();
         });
         if changed {
            ctx.appearance_settings.clamp();
            self.dirty = true;
         }

         let mut changed = false;
         Self::row(ui, row_size, &q_mark_text, BURST_MS_TIP, |ui| {
            ui.label(RichText::new("Burst length").size(theme.typography.normal));
            changed = ui
               .add(
                  egui::Slider::new(
                     &mut ctx.appearance_settings.hold.burst_ms,
                     MIN_BURST_MS..=MAX_BURST_MS,
                  )
                  .step_by(10.0)
                  .suffix(" ms"),
               )
               .changed();
         });
         if changed {
            ctx.appearance_settings.clamp();
            self.dirty = true;
         }

         if ctx.appearance_settings.hold.burst == HoldBurst::Lightning {
            let mut changed = false;
            Self::row(ui, row_size, &q_mark_text, BOLTS_TIP, |ui| {
               ui.label(RichText::new("Bolts").size(theme.typography.normal));
               changed = ui
                  .add(egui::Slider::new(
                     &mut ctx.appearance_settings.hold.burst_bolts,
                     MIN_BURST_BOLTS..=MAX_BURST_BOLTS,
                  ))
                  .changed();
            });
            if changed {
               ctx.appearance_settings.clamp();
               self.dirty = true;
            }

            let mut changed = false;
            Self::row(ui, row_size, &q_mark_text, DEPTH_TIP, |ui| {
               ui.label(RichText::new("Branch depth").size(theme.typography.normal));
               changed = ui
                  .add(egui::Slider::new(
                     &mut ctx.appearance_settings.hold.burst_branch_depth,
                     MIN_BRANCH_DEPTH..=MAX_BRANCH_DEPTH,
                  ))
                  .changed();
            });
            if changed {
               ctx.appearance_settings.clamp();
               self.dirty = true;
            }

            let mut changed = false;
            Self::row(ui, row_size, &q_mark_text, THICKNESS_TIP, |ui| {
               ui.label(RichText::new("Thickness").size(theme.typography.normal));
               changed = ui
                  .add(
                     egui::Slider::new(
                        &mut ctx.appearance_settings.hold.burst_thickness,
                        MIN_THICKNESS..=MAX_THICKNESS,
                     )
                     .step_by(0.1),
                  )
                  .changed();
            });
            if changed {
               ctx.appearance_settings.clamp();
               self.dirty = true;
            }

            let mut changed = false;
            Self::row(ui, row_size, &q_mark_text, JITTER_TIP, |ui| {
               ui.label(RichText::new("Jitter").size(theme.typography.normal));
               changed = ui
                  .add(
                     egui::Slider::new(
                        &mut ctx.appearance_settings.hold.burst_jitter,
                        MIN_JITTER..=MAX_JITTER,
                     )
                     .step_by(0.01),
                  )
                  .changed();
            });
            if changed {
               ctx.appearance_settings.clamp();
               self.dirty = true;
            }
         }

         if ctx.appearance_settings.hold.burst == HoldBurst::Glow {
            let mut changed = false;
            Self::row(ui, row_size, &q_mark_text, GLOW_FROM_TIP, |ui| {
               ui.label(RichText::new("Glow radius").size(theme.typography.normal));
               changed = ui
                  .add(
                     egui::Slider::new(
                        &mut ctx.appearance_settings.hold.burst_glow_from,
                        MIN_GLOW_FROM..=MAX_GLOW_FROM,
                     )
                     .step_by(0.1),
                  )
                  .changed();
            });
            if changed {
               ctx.appearance_settings.clamp();
               self.dirty = true;
            }

            let mut changed = false;
            Self::row(ui, row_size, &q_mark_text, GLOW_TO_TIP, |ui| {
               ui.label(RichText::new("Glow reach").size(theme.typography.normal));
               changed = ui
                  .add(
                     egui::Slider::new(
                        &mut ctx.appearance_settings.hold.burst_glow_to,
                        MIN_GLOW_TO..=MAX_GLOW_TO,
                     )
                     .step_by(0.1),
                  )
                  .changed();
            });
            if changed {
               ctx.appearance_settings.clamp();
               self.dirty = true;
            }

            let mut changed = false;
            Self::row(
               ui,
               row_size,
               &q_mark_text,
               GLOW_SOFTNESS_TIP,
               |ui| {
                  ui.label(RichText::new("Softness").size(theme.typography.normal));
                  changed = ui
                     .add(
                        egui::Slider::new(
                           &mut ctx.appearance_settings.hold.burst_glow_softness,
                           MIN_GLOW_SOFTNESS..=MAX_GLOW_SOFTNESS,
                        )
                        .step_by(0.05),
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

      let preview = Self::preview_settings(ctx);
      let text = RichText::new("Hold to confirm").size(theme.typography.large);
      let button = Button::new(text)
         .visuals(button_visuals)
         .min_size(vec2(ui.available_width() * 0.5, theme.spacing.xl));

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
         .min_size(vec2(ui.available_width() * 0.5, theme.spacing.xl));

      if ui.add(button).clicked() {
         ctx.appearance_settings = AppearanceSettings::new();
         self.preview_confirmed = false;
         self.dirty = true;
      }

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
   fn preview_settings(ctx: &ZeusContext) -> HoldSettings {
      let mut settings = ctx.appearance_settings;
      settings.clamp();
      settings.hold
   }

   /// One setting row: the control, then the question badge that explains it.
   fn row(
      ui: &mut Ui,
      row_size: Vec2,
      q_mark_text: &RichText,
      tip: &'static str,
      add: impl FnOnce(&mut Ui),
   ) {
      let qmark = Badge::new(q_mark_text.clone(), BadgeTone::Info);
      ui.allocate_ui_with_layout(
         row_size,
         Layout::left_to_right(Align::Center),
         |ui| {
            add(ui);
            ui.add(qmark).on_hover_text(tip);
         },
      );
   }
}

fn srgb(color: Color32) -> [u8; 3] {
   let [r, g, b, _a] = color.to_array();
   [r, g, b]
}
