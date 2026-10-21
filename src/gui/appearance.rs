//! Appearance settings: hold-to-confirm, its completion burst, and the app-wide
//! ambient particles.

use crate::core::context::{appearance_settings_dir, theme_kind_dir};
use crate::core::types::{
   AMBIENT_LIGHTNING_INTERVAL_SECS, DEFAULT_HOLD_SECONDS, MAX_AMBIENT_BOLTS,
   MAX_AMBIENT_BRANCH_DEPTH, MAX_AMBIENT_DURATION_MS, MAX_AMBIENT_FLASH_INTENSITY,
   MAX_AMBIENT_INTERVAL_SECS, MAX_AMBIENT_JITTER, MAX_AMBIENT_LIGHT_DENSITY,
   MAX_AMBIENT_LIGHT_FLICKER, MAX_AMBIENT_LIGHT_INTENSITY, MAX_AMBIENT_LIGHT_LIFETIME,
   MAX_AMBIENT_LIGHT_RADIUS, MAX_AMBIENT_SNOW_DENSITY, MAX_AMBIENT_SNOW_SIZE,
   MAX_AMBIENT_SNOW_SPEED, MAX_AMBIENT_THICKNESS, MAX_AMBIENT_WIND_X, MAX_AMBIENT_WIND_Y,
   MAX_BRANCH_DEPTH, MAX_BURST_BOLTS, MAX_BURST_INTENSITY, MAX_BURST_MS, MAX_GLOW_FROM,
   MAX_GLOW_SOFTNESS, MAX_GLOW_TO, MAX_HOLD_SECONDS, MAX_JITTER, MAX_THICKNESS, MIN_AMBIENT_BOLTS,
   MIN_AMBIENT_BRANCH_DEPTH, MIN_AMBIENT_DURATION_MS, MIN_AMBIENT_FLASH_INTENSITY,
   MIN_AMBIENT_INTERVAL_SECS, MIN_AMBIENT_JITTER, MIN_AMBIENT_LIGHT_DENSITY,
   MIN_AMBIENT_LIGHT_FLICKER, MIN_AMBIENT_LIGHT_INTENSITY, MIN_AMBIENT_LIGHT_LIFETIME,
   MIN_AMBIENT_LIGHT_RADIUS, MIN_AMBIENT_SNOW_DENSITY, MIN_AMBIENT_SNOW_SIZE,
   MIN_AMBIENT_SNOW_SPEED, MIN_AMBIENT_THICKNESS, MIN_AMBIENT_WIND_X, MIN_AMBIENT_WIND_Y,
   MIN_BRANCH_DEPTH, MIN_BURST_BOLTS, MIN_BURST_INTENSITY, MIN_BURST_MS, MIN_GLOW_FROM,
   MIN_GLOW_SOFTNESS, MIN_GLOW_TO, MIN_HOLD_SECONDS, MIN_JITTER, MIN_THICKNESS, default_true,
};
use crate::utils::write_private;
use egui::Color32;
use egui_elements::theme::ThemeKind;
use egui_elements::{
   HoldBurst, HoldSettings, LightSettings, LightningSettings, ParticleLayer, SnowSettings,
};
use serde::{Deserialize, Serialize};

/// Alpha of the second fill colour when the user has not picked one: the theme
/// accent at 80, so the sweep fades to roughly 31 % opacity instead of jumping
/// to a solid colour.
pub const SECOND_COLOUR_ALPHA: u8 = 80;

/// The one ambient particle effect Zeus paints.
///
/// The crate has three independent `active` flags; Zeus replaces them with this
/// single choice and derives the flags in [`AppearanceSettings::clamp`], so
/// "only one ambient particle" is an invariant of the loaded file too, not just
/// of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmbientKind {
   Snow,
   Glow,
   Lightning,
}

impl AmbientKind {
   /// Choices in the Appearance page's combo order.
   pub const ALL: [Self; 3] = [Self::Snow, Self::Glow, Self::Lightning];

   pub fn label(self) -> &'static str {
      match self {
         Self::Snow => "Snow",
         Self::Glow => "Glow",
         Self::Lightning => "Lightning",
      }
   }
}

impl Default for AmbientKind {
   /// Snow is the calmest first impression: slow, small flakes rather than
   /// drifting motes or a strike every few seconds.
   fn default() -> Self {
      Self::Snow
   }
}

/// Appearance settings: hold-to-confirm, its completion burst, and the app-wide
/// ambient particles.
///
/// Persisted as the unencrypted `appearance_settings.json`. Because the file is
/// not sealed, every value is bounded: [`Self::clamp`] runs on load, when the
/// settings page opens, on every save, and on every [`Self::button_settings`]
/// copy — so a hand-edited file can never drive a slider or a particle effect
/// out of its supported range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppearanceSettings {
   /// When true, the affirmative button in the tx and sign windows requires a
   /// hold. Off by default: the feature is opt-in.
   #[serde(default)]
   pub hold_to_confirm: bool,
   /// Whether the sweep shifts to [`Self::second_colour`] as the hold fills.
   ///
   /// On by default, and the reason this is a separate switch: the default
   /// second colour is the theme accent at [`SECOND_COLOUR_ALPHA`], which is not
   /// known when the file loads.
   #[serde(default = "default_true")]
   pub shift_second_colour: bool,
   /// Colour the sweep shifts to, or `None` for the accent at
   /// [`SECOND_COLOUR_ALPHA`].
   #[serde(default)]
   pub second_colour: Option<Color32>,
   /// The crate's knobs for the hold sweep and the completion burst.
   #[serde(default)]
   pub hold: HoldSettings,
   /// Master switch for the app-wide ambient particles. Off by default.
   #[serde(default)]
   pub ambient_enabled: bool,
   /// Which ambient particle effect is drawn. [`Self::clamp`] derives the
   /// crate's three `active` flags from this.
   #[serde(default)]
   pub ambient_kind: AmbientKind,
   /// Snow knobs for the app-wide ambient pass.
   #[serde(default)]
   pub snow: SnowSettings,
   /// Glow (light) knobs for the app-wide ambient pass.
   #[serde(default)]
   pub light: LightSettings,
   /// Lightning knobs for the app-wide ambient pass.
   #[serde(default)]
   pub lightning: LightningSettings,
}

impl AppearanceSettings {
   /// Zeus's appearance defaults.
   ///
   /// The burst values differ deliberately from the crate's: the crate defaults
   /// to a soft [`HoldBurst::Glow`], while Zeus ships a lightning-first profile
   /// (1500 ms, two bolts, no branches, a thin 1.5 px core and a light 0.1
   /// jitter). Every value is inside the clamps in [`Self::clamp`], and
   /// `burst_intensity` stays `1.0` with `burst_color = None` so the burst keeps
   /// the fill's colour.
   ///
   /// The ambient kinds keep the crate's knobs (including `color`), with four
   /// choices stated explicitly so a reader can see them, and why each exists,
   /// in one place:
   ///
   /// 1. `open` starts `false` (the crate's default too — its example is what
   ///    opens the window): Zeus opens each kind's "More…" window only on
   ///    demand, never at startup.
   /// 2. `layer` is [`ParticleLayer::Foreground`] (the crate's current
   ///    default), pinned because Zeus's panels paint opaque backgrounds that
   ///    would hide a `Background` effect.
   /// 3. Lightning's `ambient_interval_secs` is
   ///    [`AMBIENT_LIGHTNING_INTERVAL_SECS`] — the one value that differs from
   ///    the crate's default: `0.0` schedules no ambient strikes, so without it
   ///    the ambient's lightning would be `active` and never fire while snow
   ///    and glow do. Six seconds is a deliberate "ambient, not a light show"
   ///    choice, and the page exposes it.
   /// 4. Only [`Self::ambient_kind`]'s effect is `active` (snow, by default)
   ///    and the other two start off, whereas the crate defaults all three to
   ///    `active`. This is the fourth deliberate deviation: Zeus paints one
   ///    ambient particle, Snow is the calmest first impression, and
   ///    [`Self::clamp`] derives the flags so a hand-edited file cannot switch
   ///    two on at once.
   ///
   /// Every other ambient knob is the crate's default.
   pub fn new() -> Self {
      let mut settings = Self {
         hold_to_confirm: false,
         shift_second_colour: true,
         second_colour: None,
         hold: HoldSettings {
            open: false,
            seconds: DEFAULT_HOLD_SECONDS,
            burst: HoldBurst::Lightning,
            burst_ms: 1500.0,
            burst_bolts: 2,
            burst_branch_depth: 0,
            burst_thickness: 1.5,
            burst_jitter: 0.1,
            ..HoldSettings::default()
         },
         ambient_enabled: false,
         ambient_kind: AmbientKind::Snow,
         snow: SnowSettings {
            open: false,
            layer: ParticleLayer::Foreground,
            ..SnowSettings::default()
         },
         light: LightSettings {
            open: false,
            layer: ParticleLayer::Foreground,
            ..LightSettings::default()
         },
         lightning: LightningSettings {
            open: false,
            layer: ParticleLayer::Foreground,
            ambient_interval_secs: AMBIENT_LIGHTNING_INTERVAL_SECS,
            ..LightningSettings::default()
         },
      };
      settings.apply_ambient_kind();
      settings
   }

   /// Pull every numeric knob back into its supported range.
   ///
   /// The crate's `open` flags are deliberately left alone here: they are
   /// transient window state, and `save()` runs on every slider change, so
   /// clearing them in `clamp` would close a "More…" window mid-edit. Loading
   /// and saving clear them instead — see [`Self::close_settings_windows`].
   /// Also keeps each min/max pair ordered, as the crate's own body UIs do, and
   /// the glow end radius at or above the start radius. Finally it derives the
   /// crate's three `active` flags from [`Self::ambient_kind`], so exactly one
   /// ambient effect is active in the loaded file.
   pub fn clamp(&mut self) {
      self.hold.seconds = self.hold.seconds.clamp(MIN_HOLD_SECONDS, MAX_HOLD_SECONDS);
      self.hold.burst_ms = self.hold.burst_ms.clamp(MIN_BURST_MS, MAX_BURST_MS);
      self.hold.burst_intensity =
         self.hold.burst_intensity.clamp(MIN_BURST_INTENSITY, MAX_BURST_INTENSITY);
      self.hold.burst_bolts = self.hold.burst_bolts.clamp(MIN_BURST_BOLTS, MAX_BURST_BOLTS);
      self.hold.burst_branch_depth =
         self.hold.burst_branch_depth.clamp(MIN_BRANCH_DEPTH, MAX_BRANCH_DEPTH);
      self.hold.burst_thickness = self.hold.burst_thickness.clamp(MIN_THICKNESS, MAX_THICKNESS);
      self.hold.burst_jitter = self.hold.burst_jitter.clamp(MIN_JITTER, MAX_JITTER);
      self.hold.burst_glow_from = self.hold.burst_glow_from.clamp(MIN_GLOW_FROM, MAX_GLOW_FROM);
      self.hold.burst_glow_to = self
         .hold
         .burst_glow_to
         .clamp(MIN_GLOW_TO, MAX_GLOW_TO)
         .max(self.hold.burst_glow_from);
      self.hold.burst_glow_softness =
         self.hold.burst_glow_softness.clamp(MIN_GLOW_SOFTNESS, MAX_GLOW_SOFTNESS);

      self.snow.density =
         self.snow.density.clamp(MIN_AMBIENT_SNOW_DENSITY, MAX_AMBIENT_SNOW_DENSITY);
      self.snow.size_min = self.snow.size_min.clamp(MIN_AMBIENT_SNOW_SIZE, MAX_AMBIENT_SNOW_SIZE);
      self.snow.size_max = self.snow.size_max.clamp(MIN_AMBIENT_SNOW_SIZE, MAX_AMBIENT_SNOW_SIZE);
      if self.snow.size_min > self.snow.size_max {
         self.snow.size_min = self.snow.size_max;
      }
      self.snow.speed_min =
         self.snow.speed_min.clamp(MIN_AMBIENT_SNOW_SPEED, MAX_AMBIENT_SNOW_SPEED);
      self.snow.speed_max =
         self.snow.speed_max.clamp(MIN_AMBIENT_SNOW_SPEED, MAX_AMBIENT_SNOW_SPEED);
      if self.snow.speed_min > self.snow.speed_max {
         self.snow.speed_min = self.snow.speed_max;
      }
      self.snow.wind.x = self.snow.wind.x.clamp(MIN_AMBIENT_WIND_X, MAX_AMBIENT_WIND_X);
      self.snow.wind.y = self.snow.wind.y.clamp(MIN_AMBIENT_WIND_Y, MAX_AMBIENT_WIND_Y);

      self.light.density = self.light.density.clamp(
         MIN_AMBIENT_LIGHT_DENSITY,
         MAX_AMBIENT_LIGHT_DENSITY,
      );
      self.light.radius_min =
         self.light.radius_min.clamp(MIN_AMBIENT_LIGHT_RADIUS, MAX_AMBIENT_LIGHT_RADIUS);
      self.light.radius_max =
         self.light.radius_max.clamp(MIN_AMBIENT_LIGHT_RADIUS, MAX_AMBIENT_LIGHT_RADIUS);
      if self.light.radius_min > self.light.radius_max {
         self.light.radius_min = self.light.radius_max;
      }
      self.light.intensity = self.light.intensity.clamp(
         MIN_AMBIENT_LIGHT_INTENSITY,
         MAX_AMBIENT_LIGHT_INTENSITY,
      );
      self.light.flicker = self.light.flicker.clamp(
         MIN_AMBIENT_LIGHT_FLICKER,
         MAX_AMBIENT_LIGHT_FLICKER,
      );
      self.light.wind.x = self.light.wind.x.clamp(MIN_AMBIENT_WIND_X, MAX_AMBIENT_WIND_X);
      self.light.wind.y = self.light.wind.y.clamp(MIN_AMBIENT_WIND_Y, MAX_AMBIENT_WIND_Y);
      self.light.lifetime_min = self.light.lifetime_min.clamp(
         MIN_AMBIENT_LIGHT_LIFETIME,
         MAX_AMBIENT_LIGHT_LIFETIME,
      );
      self.light.lifetime_max = self.light.lifetime_max.clamp(
         MIN_AMBIENT_LIGHT_LIFETIME,
         MAX_AMBIENT_LIGHT_LIFETIME,
      );
      if self.light.lifetime_min > self.light.lifetime_max {
         self.light.lifetime_min = self.light.lifetime_max;
      }

      self.lightning.bolt_count =
         self.lightning.bolt_count.clamp(MIN_AMBIENT_BOLTS, MAX_AMBIENT_BOLTS);
      self.lightning.branch_depth = self
         .lightning
         .branch_depth
         .clamp(MIN_AMBIENT_BRANCH_DEPTH, MAX_AMBIENT_BRANCH_DEPTH);
      self.lightning.thickness =
         self.lightning.thickness.clamp(MIN_AMBIENT_THICKNESS, MAX_AMBIENT_THICKNESS);
      self.lightning.jitter = self.lightning.jitter.clamp(MIN_AMBIENT_JITTER, MAX_AMBIENT_JITTER);
      self.lightning.flash_intensity = self.lightning.flash_intensity.clamp(
         MIN_AMBIENT_FLASH_INTENSITY,
         MAX_AMBIENT_FLASH_INTENSITY,
      );
      self.lightning.duration_ms = self
         .lightning
         .duration_ms
         .clamp(MIN_AMBIENT_DURATION_MS, MAX_AMBIENT_DURATION_MS);
      self.lightning.ambient_interval_secs = self.lightning.ambient_interval_secs.clamp(
         MIN_AMBIENT_INTERVAL_SECS,
         MAX_AMBIENT_INTERVAL_SECS,
      );

      self.apply_ambient_kind();
   }

   /// Derive the crate's three `active` flags from [`Self::ambient_kind`].
   ///
   /// One ambient effect at a time is a property of the loaded file, not just of
   /// the page: a hand-edited document with all three flags true normalises to
   /// the selected kind here.
   fn apply_ambient_kind(&mut self) {
      self.snow.active = self.ambient_kind == AmbientKind::Snow;
      self.light.active = self.ambient_kind == AmbientKind::Glow;
      self.lightning.active = self.ambient_kind == AmbientKind::Lightning;
   }

   /// Clear the crate's transient settings-window flags.
   ///
   /// `open` must never come back from disk: the crate's `show_window` treats it
   /// as "the user has this window up", so a persisted `true` would reopen a
   /// floating window at startup. [`Self::clamp`] deliberately leaves it alone
   /// so a save from a slider drag cannot close the window the user is editing.
   fn close_settings_windows(&mut self) {
      self.hold.open = false;
      self.snow.open = false;
      self.light.open = false;
      self.lightning.open = false;
   }

   /// The copy [`Self::save`] writes: clamped, with every transient settings
   /// window closed.
   fn persisted(&self) -> Self {
      let mut settings = self.clone();
      settings.clamp();
      settings.close_settings_windows();
      settings
   }

   /// Parse a settings document, clamp it, and close transient windows.
   ///
   /// Tests use this instead of [`Self::load_from_file`] so they never touch the
   /// real `data/` directory, which is resolved from the process cwd.
   pub(crate) fn from_json(data: &str) -> Result<Self, serde_json::Error> {
      let mut settings: Self = serde_json::from_str(data)?;
      settings.clamp();
      settings.close_settings_windows();
      Ok(settings)
   }

   pub(crate) fn to_json(&self) -> Result<String, serde_json::Error> {
      serde_json::to_string(self)
   }

   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let path = appearance_settings_dir()?;
      let data = std::fs::read_to_string(path)?;
      Ok(Self::from_json(&data)?)
   }

   pub fn save(&self) -> Result<(), anyhow::Error> {
      // Every write is clamped: the file is unencrypted and may be replaced by
      // hand, so no caller can land an out-of-range value on disk. The
      // transient `open` flags are cleared in the copy, never in `self`.
      let settings = self.persisted();
      let path = appearance_settings_dir()?;
      let data = settings.to_json()?;
      write_private(&path, data.as_bytes())?;
      Ok(())
   }

   pub fn hold_to_confirm(&self) -> bool {
      self.hold_to_confirm
   }

   pub fn set_hold_to_confirm(&mut self, enabled: bool) {
      self.hold_to_confirm = enabled;
   }

   pub fn hold_seconds(&self) -> f32 {
      self.hold.seconds
   }

   /// Set the hold duration, clamped to [`MIN_HOLD_SECONDS`]`..=`[`MAX_HOLD_SECONDS`].
   pub fn set_hold_seconds(&mut self, secs: f32) {
      self.hold.seconds = secs.clamp(MIN_HOLD_SECONDS, MAX_HOLD_SECONDS);
   }

   /// The settings a hold-to-confirm button should use, or `None` when the
   /// feature is off so the caller leaves the button untouched.
   ///
   /// The returned copy is clamped, has [`HoldSettings::open`] cleared, and has
   /// [`HoldSettings::fill_to`] resolved from [`Self::shift_second_colour`] and
   /// [`Self::second_colour`]. `accent` is the installed theme's accent, which
   /// is why this cannot be a stored colour: `None` means "the accent at
   /// [`SECOND_COLOUR_ALPHA`]" and has to be resolved at paint time.
   pub fn button_settings(&self, accent: Color32) -> Option<HoldSettings> {
      if !self.hold_to_confirm {
         return None;
      }

      Some(self.resolved_hold(accent))
   }

   /// The hold settings without the feature gate, used by the settings preview so
   /// it can demonstrate the hold while the feature is off.
   ///
   /// This is the one place the crate's [`HoldSettings::fill_to`] is built: the
   /// switch picks a second colour or the crate's solid fill (`None`).
   pub(crate) fn resolved_hold(&self, accent: Color32) -> HoldSettings {
      let mut settings = self.clone();
      settings.clamp();
      let mut hold = settings.hold;
      hold.open = false;
      hold.fill_to = if settings.shift_second_colour {
         Some(settings.second_colour.unwrap_or_else(|| accent_at(accent, SECOND_COLOUR_ALPHA)))
      } else {
         None
      };
      hold
   }
}

/// `accent` at `alpha`, in unmultiplied sRGBA.
///
/// Deliberately not `gamma_multiply_u8`: that scales the premultiplied channels
/// and leaves alpha at 255, so the sweep would reach a dim opaque colour instead
/// of fading out.
pub(crate) fn accent_at(accent: Color32, alpha: u8) -> Color32 {
   let [r, g, b, _] = accent.to_srgba_unmultiplied();
   Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

impl Default for AppearanceSettings {
   fn default() -> Self {
      Self::new()
   }
}

/// Read the persisted theme kind.
pub fn load_theme_kind() -> Result<ThemeKind, anyhow::Error> {
   let dir = theme_kind_dir()?;
   let theme_kind_str = std::fs::read_to_string(dir)?;
   let theme_kind = serde_json::from_str(&theme_kind_str)?;
   Ok(theme_kind)
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn defaults_are_lightning_with_the_second_colour_on() {
      let settings = AppearanceSettings::new();
      assert!(!settings.hold_to_confirm());
      assert_eq!(settings.hold_seconds(), 0.6);
      assert_eq!(settings.hold.burst, HoldBurst::Lightning);
      assert_eq!(settings.hold.burst_ms, 1500.0);
      assert_eq!(settings.hold.burst_bolts, 2);
      assert_eq!(settings.hold.burst_branch_depth, 0);
      assert_eq!(settings.hold.burst_thickness, 1.5);
      assert_eq!(settings.hold.burst_jitter, 0.1);
      assert!(settings.shift_second_colour);
      assert_eq!(settings.second_colour, None);
      // The burst keeps the fill's colour and the effect's own intensity.
      assert_eq!(settings.hold.burst_intensity, 1.0);
      assert_eq!(settings.hold.burst_color, None);
      assert_eq!(settings, AppearanceSettings::default());
   }

   #[test]
   fn the_second_colour_defaults_to_the_accent_at_eighty_alpha() {
      let accent = egui::Color32::from_rgb(10, 20, 30);
      let mut settings = AppearanceSettings::new();
      settings.set_hold_to_confirm(true);

      let expected = egui::Color32::from_rgba_unmultiplied(10, 20, 30, 80);
      let hold = settings.button_settings(accent).expect("the feature is on");
      assert_eq!(hold.fill_to, Some(expected));
      assert_eq!(expected.a(), 80);

      settings.second_colour = Some(egui::Color32::from_rgba_unmultiplied(1, 2, 3, 4));
      let hold = settings.button_settings(accent).expect("the feature is on");
      assert_eq!(
         hold.fill_to,
         Some(egui::Color32::from_rgba_unmultiplied(1, 2, 3, 4))
      );
   }

   #[test]
   fn turning_the_shift_off_paints_a_solid_fill() {
      let accent = egui::Color32::from_rgb(10, 20, 30);
      let mut settings = AppearanceSettings::new();
      settings.set_hold_to_confirm(true);
      settings.shift_second_colour = false;

      let hold = settings.button_settings(accent).expect("the feature is on");
      assert_eq!(hold.fill_to, None);
   }

   #[test]
   fn clamp_pulls_every_numeric_field_into_range() {
      let mut settings = AppearanceSettings {
         hold_to_confirm: true,
         shift_second_colour: true,
         second_colour: Some(egui::Color32::from_rgba_unmultiplied(1, 2, 3, 4)),
         hold: HoldSettings {
            open: true,
            seconds: 0.01,
            fill_from: None,
            fill_to: None,
            burst: egui_elements::HoldBurst::Lightning,
            burst_color: None,
            burst_intensity: -3.0,
            burst_ms: 999_999.0,
            burst_bolts: 0,
            burst_branch_depth: 99,
            burst_thickness: -1.0,
            burst_jitter: 5.0,
            burst_glow_from: 0.0,
            burst_glow_to: 99.0,
            burst_glow_softness: 0.01,
         },
         ..AppearanceSettings::new()
      };

      settings.clamp();

      // `open` is transient window state: `clamp` must leave it alone so a save
      // from a slider drag does not close the window being edited. Loading and
      // saving clear it instead.
      assert!(settings.hold.open);
      assert_eq!(settings.hold_seconds(), MIN_HOLD_SECONDS);
      assert_eq!(settings.hold.burst_ms, MAX_BURST_MS);
      assert_eq!(settings.hold.burst_intensity, MIN_BURST_INTENSITY);
      assert_eq!(settings.hold.burst_bolts, MIN_BURST_BOLTS);
      assert_eq!(settings.hold.burst_branch_depth, MAX_BRANCH_DEPTH);
      assert_eq!(settings.hold.burst_thickness, MIN_THICKNESS);
      assert_eq!(settings.hold.burst_jitter, MAX_JITTER);
      assert_eq!(settings.hold.burst_glow_from, MIN_GLOW_FROM);
      assert_eq!(settings.hold.burst_glow_to, MAX_GLOW_TO);
      assert_eq!(
         settings.hold.burst_glow_softness,
         MIN_GLOW_SOFTNESS
      );
      assert!(settings.hold.burst_glow_to >= settings.hold.burst_glow_from);

      // Every bound is reachable from the other side too.
      settings.hold.seconds = 99.0;
      settings.hold.burst_ms = -1.0;
      settings.hold.burst_intensity = 4.0;
      settings.hold.burst_bolts = 100;
      settings.hold.burst_thickness = 50.0;
      settings.hold.burst_jitter = -1.0;
      settings.hold.burst_glow_from = 9.0;
      settings.hold.burst_glow_to = 0.0;
      settings.hold.burst_glow_softness = 9.0;
      settings.clamp();

      assert_eq!(settings.hold_seconds(), MAX_HOLD_SECONDS);
      assert_eq!(settings.hold.burst_ms, MIN_BURST_MS);
      assert_eq!(settings.hold.burst_intensity, MAX_BURST_INTENSITY);
      assert_eq!(settings.hold.burst_bolts, MAX_BURST_BOLTS);
      assert_eq!(settings.hold.burst_thickness, MAX_THICKNESS);
      assert_eq!(settings.hold.burst_jitter, MIN_JITTER);
      assert_eq!(settings.hold.burst_glow_from, MAX_GLOW_FROM);
      assert_eq!(settings.hold.burst_glow_to, MAX_GLOW_FROM);
      assert_eq!(
         settings.hold.burst_glow_softness,
         MAX_GLOW_SOFTNESS
      );
      assert!(settings.hold.burst_glow_to >= settings.hold.burst_glow_from);

      // A hand-edited `0` is below the crate's slider floor, so it clamps to `80`:
      // `HoldBurst::Off`, not a zero duration, is how the burst is disabled.
      settings.hold.burst_ms = 0.0;
      settings.clamp();
      assert_eq!(settings.hold.burst_ms, MIN_BURST_MS);
      assert_eq!(MIN_BURST_MS, 80.0);

      // One ambient kind stays active, derived from `ambient_kind`: all three
      // crate flags true still normalise to the selected kind.
      settings.snow.active = true;
      settings.light.active = true;
      settings.lightning.active = true;
      settings.ambient_kind = AmbientKind::Glow;
      settings.clamp();
      assert!(!settings.snow.active);
      assert!(settings.light.active);
      assert!(!settings.lightning.active);

      // The second colour is a colour, not a range: `clamp` leaves it alone.
      assert_eq!(
         settings.second_colour,
         Some(egui::Color32::from_rgba_unmultiplied(1, 2, 3, 4))
      );
   }

   #[test]
   fn button_settings_is_none_until_enabled_and_clamps_afterwards() {
      let mut settings = AppearanceSettings::new();
      let accent = egui::Color32::from_rgb(10, 20, 30);
      assert!(settings.button_settings(accent).is_none());

      settings.set_hold_to_confirm(true);
      settings.set_hold_seconds(0.1);
      let hold = settings.button_settings(accent).expect("the feature is on");
      assert_eq!(hold.seconds, MIN_HOLD_SECONDS);
      assert!(!hold.open);

      settings.set_hold_seconds(5.0);
      let hold = settings.button_settings(accent).expect("the feature is on");
      assert_eq!(hold.seconds, MAX_HOLD_SECONDS);
      assert!(!hold.open);
   }

   #[test]
   fn round_trips_through_json() {
      let mut settings = AppearanceSettings::new();
      settings.set_hold_to_confirm(true);
      settings.hold.fill_from = Some(egui::Color32::from_rgb(1, 2, 3));
      settings.second_colour = Some(egui::Color32::from_rgba_unmultiplied(4, 5, 6, 80));
      settings.hold.burst = HoldBurst::Glow;
      settings.hold.burst_glow_to = 3.5;
      settings.ambient_kind = AmbientKind::Lightning;
      settings.clamp();

      let json = settings.to_json().expect("serialize");
      let restored = AppearanceSettings::from_json(&json).expect("deserialize");
      assert_eq!(restored, settings);
   }

   /// The whole point of `#[serde(default)]`: the crate gains a knob, the file on
   /// disk predates it, and the missing field takes its crate default.
   #[test]
   fn a_file_missing_new_fields_still_loads() {
      let settings = AppearanceSettings::from_json(
         r#"{"hold_to_confirm": true, "hold": {"burst": "Lightning"}}"#,
      )
      .expect("a partial document loads");

      assert!(settings.hold_to_confirm());
      assert_eq!(
         settings.hold,
         HoldSettings {
            burst: HoldBurst::Lightning,
            ..HoldSettings::default()
         }
      );
      // A file written before the particle combo existed defaults to Snow and
      // normalises the three crate flags to that one kind.
      assert_eq!(settings.ambient_kind, AmbientKind::Snow);
      assert!(settings.snow.active);
      assert!(!settings.light.active);
      assert!(!settings.lightning.active);
   }

   /// A file written before the shift existed has no `shift_second_colour` or
   /// `second_colour`. A plain `#[serde(default)]` would silently give `false`
   /// and turn the feature off; `default = "default_true"` keeps it on.
   #[test]
   fn a_file_written_before_the_shift_existed_still_loads() {
      let settings =
         AppearanceSettings::from_json(r#"{"hold_to_confirm": true, "hold": {"seconds": 0.7}}"#)
            .expect("a pre-shift document loads");

      assert!(settings.shift_second_colour);
      assert_eq!(settings.second_colour, None);
   }

   #[test]
   fn an_out_of_range_file_is_clamped_on_load() {
      let settings =
         AppearanceSettings::from_json(r#"{"hold": {"seconds": 9.0}}"#).expect("clamped on load");
      assert_eq!(settings.hold_seconds(), MAX_HOLD_SECONDS);

      let settings =
         AppearanceSettings::from_json(r#"{"hold": {"seconds": 0.0}}"#).expect("clamped on load");
      assert_eq!(settings.hold_seconds(), MIN_HOLD_SECONDS);
   }

   #[test]
   fn an_unknown_field_is_ignored() {
      let settings = AppearanceSettings::from_json(
         r#"{"hold_to_confirm": true, "some_future_knob": 1, "hold": {"seconds": 0.5}}"#,
      )
      .expect("unknown fields are future-compatible");

      assert!(settings.hold_to_confirm());
      assert_eq!(settings.hold_seconds(), 0.5);
   }

   /// The fourth deliberate deviation from the crate's defaults: the crate has
   /// all three ambient kinds `active`, while Zeus paints exactly one, chosen by
   /// [`AmbientKind`]. Snow is the calmest first impression and therefore the
   /// default kind; glow and lightning start off, and the page has one
   /// "Particle" combo instead of three active checkboxes.
   #[test]
   fn ambient_defaults_are_the_crates_and_off() {
      let settings = AppearanceSettings::new();

      assert!(!settings.ambient_enabled);
      assert_eq!(settings.ambient_kind, AmbientKind::Snow);
      assert_eq!(
         settings.snow,
         SnowSettings {
            open: false,
            active: true,
            layer: ParticleLayer::Foreground,
            ..SnowSettings::default()
         }
      );
      assert_eq!(
         settings.light,
         LightSettings {
            open: false,
            active: false,
            layer: ParticleLayer::Foreground,
            ..LightSettings::default()
         }
      );
      // The one ambient value that differs from the crate's: `0.0` schedules
      // no ambient strikes, so lightning would be `active` and never fire.
      // Zeus sets a real interval, and the page exposes it.
      assert_eq!(
         settings.lightning.ambient_interval_secs,
         AMBIENT_LIGHTNING_INTERVAL_SECS
      );
      assert_ne!(
         settings.lightning.ambient_interval_secs,
         LightningSettings::default().ambient_interval_secs,
         "the crate's default interval disables ambient strikes"
      );

      // Every other lightning knob is still the crate's default.
      assert_eq!(
         settings.lightning,
         LightningSettings {
            open: false,
            active: false,
            layer: ParticleLayer::Foreground,
            ambient_interval_secs: AMBIENT_LIGHTNING_INTERVAL_SECS,
            ..LightningSettings::default()
         }
      );

      // The fourth deliberate deviation: the crate defaults all three kinds to
      // `active`, while Zeus paints one. Snow is the calmest first impression,
      // so it is the default kind and the only active effect.
      assert!(settings.snow.active);
      assert!(!settings.light.active);
      assert!(!settings.lightning.active);
      assert_eq!(
         settings.ambient_kind,
         AmbientKind::default(),
         "Snow is the default kind"
      );
   }

   /// A hand-edited file can set all three crate flags, but loading normalises
   /// them to the single selected [`AmbientKind`].
   #[test]
   fn a_file_with_every_ambient_active_normalises_to_one() {
      let mut on_disk = AppearanceSettings::new();
      on_disk.ambient_kind = AmbientKind::Glow;
      on_disk.snow.active = true;
      on_disk.light.active = true;
      on_disk.lightning.active = true;

      let loaded = AppearanceSettings::from_json(&on_disk.to_json().expect("serialize"))
         .expect("a document with every kind active loads");

      assert_eq!(loaded.ambient_kind, AmbientKind::Glow);
      assert!(!loaded.snow.active);
      assert!(loaded.light.active);
      assert!(!loaded.lightning.active);
   }

   #[test]
   fn ambient_knobs_are_clamped_to_the_crates_sliders() {
      let mut settings = AppearanceSettings::new();

      settings.snow.density = usize::MAX;
      settings.snow.size_min = -50.0;
      settings.snow.size_max = 500.0;
      settings.snow.speed_min = -10.0;
      settings.snow.speed_max = 10_000.0;
      settings.snow.wind = egui::vec2(-9_000.0, 9_000.0);
      settings.light.density = usize::MAX;
      settings.light.radius_min = 0.0;
      settings.light.radius_max = 1_000.0;
      settings.light.intensity = -1.0;
      settings.light.flicker = 9.0;
      settings.light.wind = egui::vec2(9_000.0, -9_000.0);
      settings.light.lifetime_min = 0.0;
      settings.light.lifetime_max = 5_000.0;
      settings.lightning.bolt_count = 99;
      settings.lightning.branch_depth = 99;
      settings.lightning.thickness = 0.0;
      settings.lightning.jitter = 9.0;
      settings.lightning.flash_intensity = 2.0;
      settings.lightning.duration_ms = 0.0;
      settings.lightning.ambient_interval_secs = -1.0;

      settings.clamp();

      assert_eq!(settings.snow.density, MAX_AMBIENT_SNOW_DENSITY);
      assert!((MIN_AMBIENT_SNOW_SIZE..=MAX_AMBIENT_SNOW_SIZE).contains(&settings.snow.size_min));
      assert!((MIN_AMBIENT_SNOW_SIZE..=MAX_AMBIENT_SNOW_SIZE).contains(&settings.snow.size_max));
      assert!(settings.snow.size_min <= settings.snow.size_max);
      assert!((MIN_AMBIENT_SNOW_SPEED..=MAX_AMBIENT_SNOW_SPEED).contains(&settings.snow.speed_min));
      assert!((MIN_AMBIENT_SNOW_SPEED..=MAX_AMBIENT_SNOW_SPEED).contains(&settings.snow.speed_max));
      assert!(settings.snow.speed_min <= settings.snow.speed_max);
      assert!((MIN_AMBIENT_WIND_X..=MAX_AMBIENT_WIND_X).contains(&settings.snow.wind.x));
      assert!((MIN_AMBIENT_WIND_Y..=MAX_AMBIENT_WIND_Y).contains(&settings.snow.wind.y));

      assert_eq!(settings.light.density, MAX_AMBIENT_LIGHT_DENSITY);
      assert!(
         (MIN_AMBIENT_LIGHT_RADIUS..=MAX_AMBIENT_LIGHT_RADIUS).contains(&settings.light.radius_min)
      );
      assert!(
         (MIN_AMBIENT_LIGHT_RADIUS..=MAX_AMBIENT_LIGHT_RADIUS).contains(&settings.light.radius_max)
      );
      assert!(settings.light.radius_min <= settings.light.radius_max);
      assert!(
         (MIN_AMBIENT_LIGHT_INTENSITY..=MAX_AMBIENT_LIGHT_INTENSITY)
            .contains(&settings.light.intensity)
      );
      assert!(
         (MIN_AMBIENT_LIGHT_FLICKER..=MAX_AMBIENT_LIGHT_FLICKER).contains(&settings.light.flicker)
      );
      assert!((MIN_AMBIENT_WIND_X..=MAX_AMBIENT_WIND_X).contains(&settings.light.wind.x));
      assert!((MIN_AMBIENT_WIND_Y..=MAX_AMBIENT_WIND_Y).contains(&settings.light.wind.y));
      assert!(
         (MIN_AMBIENT_LIGHT_LIFETIME..=MAX_AMBIENT_LIGHT_LIFETIME)
            .contains(&settings.light.lifetime_min)
      );
      assert!(
         (MIN_AMBIENT_LIGHT_LIFETIME..=MAX_AMBIENT_LIGHT_LIFETIME)
            .contains(&settings.light.lifetime_max)
      );
      assert!(settings.light.lifetime_min <= settings.light.lifetime_max);

      assert_eq!(settings.lightning.bolt_count, MAX_AMBIENT_BOLTS);
      assert_eq!(
         settings.lightning.branch_depth,
         MAX_AMBIENT_BRANCH_DEPTH
      );
      assert!(
         (MIN_AMBIENT_THICKNESS..=MAX_AMBIENT_THICKNESS).contains(&settings.lightning.thickness)
      );
      assert!((MIN_AMBIENT_JITTER..=MAX_AMBIENT_JITTER).contains(&settings.lightning.jitter));
      assert!(
         (MIN_AMBIENT_FLASH_INTENSITY..=MAX_AMBIENT_FLASH_INTENSITY)
            .contains(&settings.lightning.flash_intensity)
      );
      assert!(
         (MIN_AMBIENT_DURATION_MS..=MAX_AMBIENT_DURATION_MS)
            .contains(&settings.lightning.duration_ms)
      );
      assert!(
         (MIN_AMBIENT_INTERVAL_SECS..=MAX_AMBIENT_INTERVAL_SECS)
            .contains(&settings.lightning.ambient_interval_secs)
      );

      // `0` is a valid interval: the crate treats it as "ambient scheduler off".
      settings.lightning.ambient_interval_secs = 0.0;
      settings.clamp();
      assert_eq!(settings.lightning.ambient_interval_secs, 0.0);
   }

   #[test]
   fn the_settings_windows_are_closed_on_load_and_save() {
      // A hand-edited file with every transient window flagged open. The
      // particle settings require all their fields, so serialize a full
      // document and flip the flags rather than hand-writing a partial one.
      let mut on_disk = AppearanceSettings::new();
      on_disk.hold.open = true;
      on_disk.snow.open = true;
      on_disk.light.open = true;
      on_disk.lightning.open = true;
      let loaded = AppearanceSettings::from_json(&on_disk.to_json().expect("serialize"))
         .expect("a document with every window open loads");

      assert!(!loaded.hold.open);
      assert!(!loaded.snow.open);
      assert!(!loaded.light.open);
      assert!(!loaded.lightning.open);

      let mut live = AppearanceSettings::new();
      live.hold.open = true;
      live.snow.open = true;
      live.light.open = true;
      live.lightning.open = true;

      // A slider drag saves while the "More…" window is up: `clamp` must not
      // close it.
      live.clamp();
      assert!(live.hold.open);
      assert!(live.snow.open);
      assert!(live.light.open);
      assert!(live.lightning.open);

      let saved = live.persisted();
      assert!(!saved.hold.open);
      assert!(!saved.snow.open);
      assert!(!saved.light.open);
      assert!(!saved.lightning.open);
      assert!(!saved.to_json().expect("serialize the saved copy").contains("\"open\":true"));
   }
}
