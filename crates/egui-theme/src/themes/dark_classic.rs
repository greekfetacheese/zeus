use super::super::{TextSizes, Theme, FrameVisuals, ThemeColors, ThemeKind};
use egui::{
   Color32, CornerRadius, Frame, Margin, Shadow, Stroke, Style, Visuals,
   style::{Selection, WidgetVisuals, Widgets},
};

// Background

const BG_DARK: Color32 = Color32::from_rgba_premultiplied(17, 17, 18, 255);
const BG: Color32 = Color32::from_rgba_premultiplied(24, 24, 26, 255);
const BG_LIGHT: Color32 = Color32::from_rgba_premultiplied(31, 31, 34, 255);
const BG_LIGHT2: Color32 = Color32::from_rgba_premultiplied(43, 43, 48, 255);

// Text

const TEXT: Color32 = Color32::from_rgba_premultiplied(204, 204, 204, 255);
const TEXT_MUTED: Color32 = Color32::from_rgba_premultiplied(127, 127, 127, 255);

// Highlight

const HIGHLIGHT: Color32 = Color32::from_rgba_premultiplied(160, 160, 160, 255);

// Border

// const BORDER: Color32 = Color32::from_rgba_premultiplied(102, 102, 102, 255);
const BORDER: Color32 = Color32::from_rgba_premultiplied(46, 46, 46, 255);

// Action

const PRIMARY: Color32 = Color32::from_rgba_premultiplied(221, 152, 198, 255);
const SECONDARY: Color32 = Color32::from_rgba_premultiplied(118, 198, 157, 255);

// Alert

const ERROR: Color32 = Color32::from_rgba_premultiplied(237, 57, 57, 255);
const WARNING: Color32 = Color32::from_rgba_premultiplied(228, 128, 25, 255);
const SUCCESS: Color32 = Color32::from_rgba_premultiplied(72, 182, 120, 255);
const INFO: Color32 = Color32::from_rgba_premultiplied(195, 153, 255, 255);

/// Return this theme
pub fn theme() -> Theme {
   Theme {
      dark_mode: true,
      kind: ThemeKind::DarkClassic,
      style: style(),
      colors: colors(),
      text_sizes: text_sizes(),
      window_frame: window_frame(&colors()),
      frame1: frame1(&colors()),
      frame2: frame2(&colors()),
      frame1_visuals: frame1_visuals(&colors()),
      frame2_visuals: frame2_visuals(&colors()),
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
      border_muted: BORDER,
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

pub fn frame1_visuals(colors: &ThemeColors) -> FrameVisuals {
   FrameVisuals {
      bg_on_hover: colors.bg_light,
      bg_on_click: colors.bg_light,
      border_on_hover: (1.0, colors.border),
      border_on_click: (1.0, colors.border),
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

pub fn frame2_visuals(colors: &ThemeColors) -> FrameVisuals {
   FrameVisuals {
      bg_on_hover: colors.bg_light,
      bg_on_click: colors.bg_light,
      border_on_hover: (1.0, colors.border),
      border_on_click: (1.0, colors.border),
   }
}

fn style() -> Style {
   let widgets = widgets(colors());
   let visuals = visuals(widgets, &colors());
   Style {
      visuals,
      animation_time: 0.3,
      ..Default::default()
   }
}

fn visuals(widgets: Widgets, colors: &ThemeColors) -> Visuals {
   Visuals {
      dark_mode: true,
      override_text_color: Some(TEXT),
      widgets,
      selection: Selection {
         bg_fill: colors.bg_light2, // affects selected text color, combox selected item bg
         stroke: Stroke::new(1.0, colors.highlight), // also affects TextEdit border color when active
      },
      hyperlink_color: colors.info,
      faint_bg_color: colors.bg_dark,
      extreme_bg_color: colors.bg,
      code_bg_color: colors.bg_dark,
      warn_fg_color: colors.warning,
      error_fg_color: colors.error,
      window_corner_radius: CornerRadius::same(6),
      window_shadow: Shadow {
         offset: (0, 0).into(),
         blur: 12,
         spread: 1,
         color: Color32::from_rgba_premultiplied(0, 0, 0, 255),
      },
      window_fill: colors.bg,
      window_stroke: Stroke::new(1.0, colors.border),
      panel_fill: colors.bg_dark,
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

   // Set inactive bg to border color
   // Because widgets like sliders dont get a border and it will not distinguish
   // from the bg color
   let mut inactive_visuals = base_visuals.clone();
   inactive_visuals.bg_fill = colors.border;

   Widgets {
      noninteractive: non_interactive_base,
      inactive: inactive_visuals,
      hovered: WidgetVisuals {
         bg_fill: colors.bg_light,
         weak_bg_fill: colors.bg_dark,
         bg_stroke: Stroke::new(1.0, colors.highlight),
         ..base_visuals
      },
      active: WidgetVisuals {
         bg_fill: colors.bg_dark,
         weak_bg_fill: colors.bg_dark,
         bg_stroke: Stroke::new(1.0, colors.border),
         ..base_visuals
      },
      open: WidgetVisuals {
         bg_fill: colors.bg_dark,
         weak_bg_fill: colors.bg_dark,
         bg_stroke: Stroke::new(1.0, colors.border),
         ..base_visuals
      },
   }
}
