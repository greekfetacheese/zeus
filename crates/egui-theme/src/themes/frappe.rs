use super::super::{FrameVisuals, TextSizes, Theme, ThemeColors, ThemeKind};
use egui::{
   Color32, CornerRadius, Frame, Margin, Shadow, Stroke, Style, Visuals,
   style::{Selection, WidgetVisuals, Widgets},
};

// Catppuccin Frappe Palette
pub const ROSEWATER: Color32 = Color32::from_rgb(242, 213, 207);
pub const FLAMINGO: Color32 = Color32::from_rgb(238, 190, 190);
pub const MAUVE: Color32 = Color32::from_rgb(202, 158, 230);
pub const PINK: Color32 = Color32::from_rgb(244, 184, 228);
pub const MAROON: Color32 = Color32::from_rgb(235, 151, 154);
pub const RED: Color32 = Color32::from_rgb(231, 130, 132);
pub const PEACH: Color32 = Color32::from_rgb(239, 159, 118);
pub const YELLOW: Color32 = Color32::from_rgb(229, 200, 144);
pub const GREEN: Color32 = Color32::from_rgb(166, 209, 137);
pub const TEAL: Color32 = Color32::from_rgb(147, 205, 209);
pub const SAPPHIRE: Color32 = Color32::from_rgb(133, 193, 220);
pub const SKY: Color32 = Color32::from_rgb(154, 214, 229);
pub const BLUE: Color32 = Color32::from_rgb(140, 170, 238);
pub const LAVENDER: Color32 = Color32::from_rgb(186, 187, 241);

pub const TEXT: Color32 = Color32::from_rgb(198, 208, 245);
pub const SUBTEXT1: Color32 = Color32::from_rgb(181, 191, 226);
pub const SUBTEXT0: Color32 = Color32::from_rgb(165, 173, 206);

pub const OVERLAY2: Color32 = Color32::from_rgb(148, 155, 188);
pub const OVERLAY1: Color32 = Color32::from_rgb(132, 138, 170);
pub const OVERLAY0: Color32 = Color32::from_rgb(115, 121, 151);

pub const SURFACE2: Color32 = Color32::from_rgb(99, 104, 133);
pub const SURFACE1: Color32 = Color32::from_rgb(82, 87, 114);
pub const SURFACE0: Color32 = Color32::from_rgb(65, 69, 89);

pub const BASE: Color32 = Color32::from_rgb(48, 52, 70);
pub const MANTLE: Color32 = Color32::from_rgb(41, 44, 60);
pub const CRUST: Color32 = Color32::from_rgb(35, 38, 52);

/// Return this theme
pub fn theme() -> Theme {
   Theme {
      kind: ThemeKind::Frappe,
      style: style(),
      colors: colors(),
      text_sizes: text_sizes(),
      window_frame: window_frame(),
      frame1: frame1(),
      frame2: frame2(),
      frame1_visuals: frame1_visuals(),
      frame2_visuals: frame2_visuals(),
   }
}

fn colors() -> ThemeColors {
   ThemeColors {
      bg_color: BASE,
      secondary_bg_color: MANTLE,
      extreme_bg_color: CRUST,
      window_fill: BASE,
      highlight1: MAUVE,
      highlight2: LAVENDER,
      overlay_color: Color32::from_rgba_premultiplied(0, 0, 0, 150),
      text_color: TEXT,
      text_secondary: SUBTEXT0,
      text_edit_bg: CRUST,
      error_color: RED,
      success_color: GREEN,
      hyperlink_color: BLUE,
      button_bg: SURFACE0,
      widget_bg_color: SURFACE1,
      widget_bg_color_hover: MAUVE,
      widget_bg_color_click: PINK,
      widget_bg_color_open: SURFACE2,
      border_color_idle: SURFACE0,
      border_color_click: PINK,
      border_color_hover: MAUVE,
      border_color_open: LAVENDER,
   }
}

fn text_sizes() -> TextSizes {
   TextSizes::new(12.0, 14.0, 16.0, 18.0, 20.0, 26.0)
}

fn window_frame() -> Frame {
   Frame {
      corner_radius: CornerRadius::same(6),
      fill: BASE,
      stroke: Stroke::new(1.0, CRUST),
      ..Default::default()
   }
}

fn frame1() -> Frame {
   Frame {
      corner_radius: CornerRadius::same(6),
      inner_margin: Margin::same(12),
      fill: MANTLE,
      stroke: Stroke::new(1.0, CRUST),
      ..Default::default()
   }
}

fn frame2() -> Frame {
   Frame {
      corner_radius: CornerRadius::same(6),
      inner_margin: Margin::same(10),
      fill: SURFACE0,
      stroke: Stroke::NONE,
      ..Default::default()
   }
}

fn frame1_visuals() -> FrameVisuals {
   FrameVisuals {
      bg_on_hover: SURFACE1,
      bg_on_click: SURFACE2,
      border_on_hover: (1.0, MAUVE),
      border_on_click: (1.0, PINK),
   }
}

fn frame2_visuals() -> FrameVisuals {
   frame1_visuals()
}

fn style() -> Style {
   Style {
      visuals: visuals(widgets(colors())),
      ..Default::default()
   }
}

fn visuals(widgets: Widgets) -> Visuals {
   let colors = colors();
   Visuals {
      dark_mode: true,
      override_text_color: Some(colors.text_color),
      widgets,
      selection: Selection {
         bg_fill: LAVENDER,
         stroke: Stroke::NONE,
      },
      hyperlink_color: colors.hyperlink_color,
      faint_bg_color: CRUST,
      extreme_bg_color: MANTLE,
      code_bg_color: MANTLE,
      warn_fg_color: PEACH,
      error_fg_color: RED,
      window_corner_radius: CornerRadius::same(6),
      window_shadow: Shadow {
         offset: (0, 0).into(),
         blur: 20,
         spread: 0,
         color: Color32::from_black_alpha(80),
      },
      window_fill: colors.window_fill,
      window_stroke: Stroke::new(1.0, CRUST),
      panel_fill: colors.bg_color,
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
