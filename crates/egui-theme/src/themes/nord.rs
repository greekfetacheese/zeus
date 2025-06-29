use super::super::{FrameVisuals, TextSizes, Theme, ThemeColors, ThemeKind};
use egui::{
   Color32, CornerRadius, Frame, Margin, Shadow, Stroke, Style, Visuals,
   style::{Selection, WidgetVisuals, Widgets},
};

// Nord Palette

pub const POLAR_NIGHT_0: Color32 = Color32::from_rgb(46, 52, 64); // #2E3440
pub const POLAR_NIGHT_1: Color32 = Color32::from_rgb(59, 66, 82); // #3B4252
pub const POLAR_NIGHT_2: Color32 = Color32::from_rgb(67, 76, 94); // #434C5E
pub const POLAR_NIGHT_3: Color32 = Color32::from_rgb(76, 86, 106); // #4C566A

pub const SNOW_STORM_0: Color32 = Color32::from_rgb(216, 222, 233); // #D8DEE9
pub const SNOW_STORM_1: Color32 = Color32::from_rgb(229, 233, 240); // #E5E9F0
pub const SNOW_STORM_2: Color32 = Color32::from_rgb(236, 239, 244); // #ECEFF4

pub const FROST_0: Color32 = Color32::from_rgb(143, 188, 187); // #8FBCBB
pub const FROST_1: Color32 = Color32::from_rgb(136, 192, 208); // #88C0D0
pub const FROST_2: Color32 = Color32::from_rgb(129, 161, 193); // #81A1C1
pub const FROST_3: Color32 = Color32::from_rgb(94, 129, 172); // #5E81AC

pub const AURORA_RED: Color32 = Color32::from_rgb(191, 97, 106); // #BF616A
pub const AURORA_ORANGE: Color32 = Color32::from_rgb(208, 135, 112); // #D08770
pub const AURORA_YELLOW: Color32 = Color32::from_rgb(235, 203, 139); // #EBCB8B
pub const AURORA_GREEN: Color32 = Color32::from_rgb(163, 190, 140); // #A3BE8C
pub const AURORA_PURPLE: Color32 = Color32::from_rgb(180, 142, 173); // #B48EAD

pub const ERROR: Color32 = Color32::from_rgb(216, 43, 61);
pub const SUCCESS: Color32 = Color32::from_rgb(106, 173, 30);

/// Return this theme
pub fn theme() -> Theme {
   Theme {
      kind: ThemeKind::Nord,
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
      bg_color: POLAR_NIGHT_0,
      secondary_bg_color: POLAR_NIGHT_1,
      extreme_bg_color: POLAR_NIGHT_2,
      window_fill: POLAR_NIGHT_1,
      highlight1: FROST_2,
      highlight2: FROST_1,
      overlay_color: Color32::from_rgba_premultiplied(0, 0, 0, 150),
      text_color: SNOW_STORM_2,
      text_secondary: SNOW_STORM_0,
      text_edit_bg: POLAR_NIGHT_3,
      error_color: ERROR,
      success_color: SUCCESS,
      hyperlink_color: FROST_1,
      button_bg: POLAR_NIGHT_3,
      widget_bg_color: POLAR_NIGHT_2,
      widget_bg_color_hover: FROST_3,
      widget_bg_color_click: FROST_2,
      widget_bg_color_open: POLAR_NIGHT_3,
      border_color_idle: POLAR_NIGHT_3,
      border_color_click: FROST_2,
      border_color_hover: FROST_3,
      border_color_open: FROST_0,
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
      fill: POLAR_NIGHT_1,
      stroke: Stroke::new(1.0, POLAR_NIGHT_2),
      shadow: Shadow::NONE,
      ..Default::default()
   }
}

/// Frame for nested elements, like individual list items.
fn frame2() -> Frame {
   Frame {
      corner_radius: CornerRadius::same(6),
      inner_margin: Margin::same(10),
      fill: POLAR_NIGHT_2, // Contrasts with frame1
      stroke: Stroke::NONE,
      ..Default::default()
   }
}

/// Visuals for interactive items within `frame1`, like wallet cards.
fn frame1_visuals() -> FrameVisuals {
   FrameVisuals {
      bg_on_hover: POLAR_NIGHT_3,
      bg_on_click: FROST_3,
      border_on_hover: (1.0, FROST_1),
      border_on_click: (1.0, FROST_2),
   }
}

/// Visuals for interactive elements inside a `frame2`.
fn frame2_visuals() -> FrameVisuals {
   FrameVisuals {
      bg_on_hover: POLAR_NIGHT_1,
      bg_on_click: FROST_3,
      border_on_hover: (1.0, FROST_1),
      border_on_click: (1.0, FROST_2),
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
      override_text_color: Some(SNOW_STORM_2),
      widgets,
      selection: Selection::default(),
      hyperlink_color: FROST_1,
      faint_bg_color: POLAR_NIGHT_0,
      extreme_bg_color: POLAR_NIGHT_3,
      code_bg_color: POLAR_NIGHT_0,
      warn_fg_color: AURORA_YELLOW,
      error_fg_color: ERROR,
      window_corner_radius: CornerRadius::same(6),
      window_shadow: Shadow {
         offset: (0, 0).into(),
         blur: 20,
         spread: 0,
         color: Color32::from_black_alpha(50),
      },
      window_fill: POLAR_NIGHT_1,
      window_stroke: Stroke::new(1.0, POLAR_NIGHT_3),
      panel_fill: POLAR_NIGHT_0,
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

   let mut non_interactive_base = base_visuals.clone();
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