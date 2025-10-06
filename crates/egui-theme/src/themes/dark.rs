use super::super::{TextSizes, Theme, ThemeColors, ThemeKind};
use egui::{
   Color32, CornerRadius, Frame, Margin, Shadow, Stroke, Style, Visuals,
   style::{Selection, WidgetVisuals, Widgets},
};

// Background

const BG_DARK: Color32 = Color32::from_rgba_premultiplied(25, 25, 25, 255);
const BG: Color32 = Color32::from_rgba_premultiplied(10, 10, 10, 255);
const BG_LIGHT: Color32 = Color32::from_rgba_premultiplied(23, 23, 23, 255);
const BG_LIGHT2: Color32 = Color32::from_rgba_premultiplied(23, 23, 23, 255);

// Text

const TEXT: Color32 = Color32::from_rgba_premultiplied(242, 242, 242, 255);
const TEXT_MUTED: Color32 = Color32::from_rgba_premultiplied(179, 179, 179, 255);

// Highlight

const HIGHLIGHT: Color32 = Color32::from_rgba_premultiplied(98, 98, 98, 255);

// Border

const BORDER: Color32 = Color32::from_rgba_premultiplied(71, 71, 71, 255);
const BORDER_MUTED: Color32 = Color32::from_rgba_premultiplied(46, 46, 46, 255);

// Action

const PRIMARY: Color32 = Color32::from_rgba_premultiplied(221, 152, 198, 255);
const SECONDARY: Color32 = Color32::from_rgba_premultiplied(118, 198, 157, 255);

// Alert

const ERROR: Color32 = Color32::from_rgba_premultiplied(191, 60, 37, 255);
const WARNING: Color32 = Color32::from_rgba_premultiplied(173, 102, 30, 255);
const SUCCESS: Color32 = Color32::from_rgba_premultiplied(104, 175, 135, 255);
const INFO: Color32 = Color32::from_rgba_premultiplied(53, 110, 200, 255);

/// Return this theme
pub fn theme() -> Theme {
   Theme {
      dark_mode: true,
      kind: ThemeKind::Dark,
      style: style(),
      colors: colors(),
      text_sizes: text_sizes(),
      window_frame: window_frame(&colors()),
      frame1: frame1(&colors()),
      frame2: frame2(&colors()),
   }
}

/// Return the theme colors for this theme
fn colors() -> ThemeColors {
   ThemeColors {
      bg_dark: BG_DARK,
      bg: BG,
      bg_light: BG_LIGHT,
      bg_light2: BG_LIGHT2,
      text: TEXT,
      text_muted: TEXT_MUTED,
      highlight: HIGHLIGHT,
      border: BORDER,
      border_muted: BORDER_MUTED,
      primary: PRIMARY,
      secondary: SECONDARY,
      error: ERROR,
      warning: WARNING,
      success: SUCCESS,
      info: INFO,
   }
}

fn text_sizes() -> TextSizes {
   TextSizes::new(12.0, 14.0, 16.0, 18.0, 20.0, 26.0)
}

pub fn window_frame(colors: &ThemeColors) -> Frame {
   Frame {
      corner_radius: CornerRadius::same(6),
      fill: colors.bg_dark,
      stroke: Stroke::new(1.0, colors.bg),
      ..Default::default()
   }
}

/// Base container frame for major UI sections.
pub fn frame1(colors: &ThemeColors) -> Frame {
   Frame {
      corner_radius: CornerRadius::same(6),
      inner_margin: Margin::same(12),
      fill: colors.bg,
      stroke: Stroke::new(1.0, colors.border),
      shadow: Shadow::NONE,
      ..Default::default()
   }
}

/// Frame for nested elements, like individual list items.
pub fn frame2(colors: &ThemeColors) -> Frame {
   Frame {
      corner_radius: CornerRadius::same(6),
      inner_margin: Margin::same(10),
      outer_margin: Margin::same(10),
      fill: colors.bg_light,
      stroke: Stroke::NONE,
      ..Default::default()
   }
}

fn style() -> Style {
   Style {
      visuals: visuals(widgets(colors())),
      animation_time: 0.3,
      ..Default::default()
   }
}

fn visuals(widgets: Widgets) -> Visuals {
   Visuals {
      dark_mode: true,
      override_text_color: Some(TEXT),
      widgets,
      selection: Selection::default(),
      hyperlink_color: INFO,
      faint_bg_color: BG_DARK,
      extreme_bg_color: BG,
      code_bg_color: BG_DARK,
      warn_fg_color: WARNING,
      error_fg_color: ERROR,
      window_corner_radius: CornerRadius::same(6),
      window_shadow: Shadow {
         offset: (0, 0).into(),
         blur: 20,
         spread: 0,
         color: Color32::from_black_alpha(50),
      },
      window_fill: BG,
      window_stroke: Stroke::new(1.0, BORDER),
      panel_fill: BG_DARK,
      ..Default::default()
   }
}

fn widgets(colors: ThemeColors) -> Widgets {
   let base_visuals = WidgetVisuals {
      bg_fill: colors.bg_light,
      weak_bg_fill: colors.bg_light,
      bg_stroke: Stroke::new(1.0, colors.border),
      corner_radius: CornerRadius::same(4),
      fg_stroke: Stroke::new(1.0, colors.text),
      expansion: 0.0,
   };

   let mut non_interactive_base = base_visuals.clone();
   non_interactive_base.bg_stroke.width = 0.0;
   Widgets {
      noninteractive: non_interactive_base,
      inactive: base_visuals,
      hovered: WidgetVisuals {
         bg_fill: colors.highlight,
         weak_bg_fill: colors.highlight,
         bg_stroke: Stroke::new(1.0, colors.border),
         ..base_visuals
      },
      active: WidgetVisuals {
         bg_fill: colors.primary,
         weak_bg_fill: colors.primary,
         bg_stroke: Stroke::new(1.0, colors.border),
         ..base_visuals
      },
      open: WidgetVisuals {
         bg_fill: colors.primary,
         weak_bg_fill: colors.primary,
         bg_stroke: Stroke::new(1.0, colors.border),
         ..base_visuals
      },
   }
}
