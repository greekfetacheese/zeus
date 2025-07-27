use super::super::{FrameVisuals, TextSizes, Theme, ThemeColors, ThemeKind};
use egui::{
   Color32, CornerRadius, Frame, Margin, Shadow, Stroke, Style, Visuals,
   style::{Selection, WidgetVisuals, Widgets},
};

// Tokyo Night Color Palette

/// Main background
pub const STORM: Color32 = Color32::from_rgb(25, 27, 41);

/// Lighter background - Frame 1
pub const SURFACE: Color32 = Color32::from_rgb(31, 33, 46);

/// Darker elements - Frame 2
pub const DARK: Color32 = Color32::from_rgb(19, 20, 29);

/// Main text
pub const LIGHT_BLUE: Color32 = Color32::from_rgb(193, 205, 242);

/// Secondary text
pub const BLUE_5: Color32 = Color32::from_rgb(112, 126, 178);

/// Main accent
pub const MAGENTA: Color32 = Color32::from_rgb(187, 127, 207);

/// Links & accents
pub const BLUE: Color32 = Color32::from_rgb(122, 162, 247);

/// Other accent
pub const CYAN: Color32 = Color32::from_rgb(148, 226, 213);

/// Success
pub const GREEN: Color32 = Color32::from_rgb(158, 206, 106);

/// Darker Green
pub const GREEN_1: Color32 = Color32::from_rgb(63, 131, 117);

/// Warnings
pub const YELLOW: Color32 = Color32::from_rgb(224, 175, 104);

/// Other accent
pub const ORANGE: Color32 = Color32::from_rgb(255, 158, 104);

/// Errors
pub const RED: Color32 = Color32::from_rgb(247, 118, 142);

/// Return this theme
pub fn theme() -> Theme {
   Theme {
      kind: ThemeKind::TokyoNight,
      style: style(),
      colors: colors(),
      text_sizes: text_sizes(),
      frame1: frame1(),
      frame2: frame2(),
      frame1_visuals: frame1_visuals(),
      frame2_visuals: frame2_visuals(),
   }
}

/// Return the theme colors for this theme
fn colors() -> ThemeColors {
   ThemeColors {
      bg_color: STORM,
      secondary_bg_color: SURFACE,
      extreme_bg_color: DARK,
      window_fill: SURFACE,
      highlight1: MAGENTA,
      highlight2: BLUE,
      overlay_color: Color32::from_rgba_premultiplied(0, 0, 0, 150),
      text_color: LIGHT_BLUE,
      text_secondary: BLUE_5,
      text_edit_bg: DARK,
      error_color: RED,
      success_color: GREEN,
      hyperlink_color: BLUE,
      button_bg: SURFACE, // Subtle idle button
      widget_bg_color: DARK,
      widget_bg_color_hover: MAGENTA, // Bright, neon hover
      widget_bg_color_click: BLUE,    // Bright, neon click
      widget_bg_color_open: DARK,
      border_color_idle: SURFACE,
      border_color_click: BLUE,
      border_color_hover: MAGENTA,
      border_color_open: CYAN,
   }
}

fn text_sizes() -> TextSizes {
   TextSizes::new(12.0, 14.0, 16.0, 18.0, 20.0, 26.0)
}

/// Base container frame for major UI sections.
fn frame1() -> Frame {
   Frame {
      corner_radius: CornerRadius::same(6),
      inner_margin: Margin::same(12),
      fill: SURFACE,
      stroke: Stroke::new(1.0, DARK),
      ..Default::default()
   }
}

/// Frame for nested elements, like individual list items.
fn frame2() -> Frame {
   Frame {
      corner_radius: CornerRadius::same(6),
      inner_margin: Margin::same(10),
      fill: DARK, // Contrasts with frame1
      stroke: Stroke::NONE,
      ..Default::default()
   }
}

/// Visuals for interactive items within `frame1`, like wallet cards.
fn frame1_visuals() -> FrameVisuals {
   FrameVisuals {
      bg_on_hover: Color32::from_gray(40), // A slightly lighter version of the base
      bg_on_click: BLUE,
      border_on_hover: (1.0, MAGENTA),
      border_on_click: (1.0, BLUE),
   }
}

/// Visuals for interactive elements inside a `frame2`.
fn frame2_visuals() -> FrameVisuals {
   FrameVisuals {
      bg_on_hover: SURFACE,
      bg_on_click: BLUE,
      border_on_hover: (1.0, MAGENTA),
      border_on_click: (1.0, BLUE),
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
      override_text_color: Some(LIGHT_BLUE),
      widgets,
      selection: Selection::default(),
      hyperlink_color: BLUE,
      faint_bg_color: STORM,
      extreme_bg_color: DARK,
      code_bg_color: DARK,
      warn_fg_color: YELLOW,
      error_fg_color: RED,
      window_corner_radius: CornerRadius::same(6),
      window_shadow: Shadow {
         offset: (0, 0).into(),
         blur: 20,
         spread: 0,
         color: Color32::from_black_alpha(80),
      },
      window_fill: SURFACE,
      window_stroke: Stroke::new(1.0, DARK),
      panel_fill: STORM,
      ..Default::default()
   }
}

fn widgets(colors: ThemeColors) -> Widgets {
   let base_visuals = WidgetVisuals {
      bg_fill: colors.widget_bg_color,
      weak_bg_fill: colors.widget_bg_color,
      bg_stroke: Stroke::new(1.0, colors.border_color_idle),
      corner_radius: CornerRadius::same(4),
      fg_stroke: Stroke::new(1.0, colors.text_color),
      expansion: 0.0,
   };

   let mut non_interactive_base = base_visuals;
   non_interactive_base.bg_stroke.width = 0.0;

   Widgets {
      noninteractive: non_interactive_base,
      inactive: base_visuals,
      hovered: WidgetVisuals {
         bg_fill: colors.widget_bg_color_hover,
         weak_bg_fill: colors.widget_bg_color_hover,
         bg_stroke: Stroke::new(1.0, colors.border_color_hover),
         ..base_visuals
      },
      active: WidgetVisuals {
         bg_fill: colors.widget_bg_color_click,
         weak_bg_fill: colors.widget_bg_color_click,
         bg_stroke: Stroke::new(1.0, colors.border_color_click),
         ..base_visuals
      },
      open: WidgetVisuals {
         bg_fill: colors.widget_bg_color_open,
         weak_bg_fill: colors.widget_bg_color_open,
         bg_stroke: Stroke::new(1.0, colors.border_color_open),
         ..base_visuals
      },
   }
}
