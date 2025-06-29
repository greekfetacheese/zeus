use egui::{Color32, Frame, style::Style};

const PANIC_MSG: &str = "Custom theme not supported, use Theme::from_custom() instead";

pub mod editor;
pub mod themes;
pub mod utils;

pub use editor::ThemeEditor;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ThemeKind {
   /// https://catppuccin.com
   Frappe,

   /// https://catppuccin.com
   Latte,

   /// https://github.com/tokyo-night
   TokyoNight,

   /// https://www.nordtheme.com/
   Nord,

   /// A custom theme
   Custom,
}

impl ThemeKind {
   pub fn to_str(&self) -> &str {
      match self {
         ThemeKind::Frappe => "Frappe",
         ThemeKind::Latte => "Latte",
         ThemeKind::TokyoNight => "TokyoNight",
         ThemeKind::Nord => "Nord",
         ThemeKind::Custom => "Custom",
      }
   }

   pub fn to_vec() -> Vec<Self> {
      vec![Self::Frappe, Self::Latte, Self::TokyoNight, Self::Nord]
   }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Theme {
   pub kind: ThemeKind,
   pub style: Style,
   pub colors: ThemeColors,
   pub text_sizes: TextSizes,
   /// Base container frame for major UI sections.
   pub frame1: Frame,
   /// Frame for nested elements, like individual list items.
   pub frame2: Frame,
   /// Visuals for interactive elements inside a `frame1`.
   pub frame1_visuals: FrameVisuals,
   /// Visuals for interactive elements inside a `frame2`.
   pub frame2_visuals: FrameVisuals,
}

impl Theme {
   /// Panics if the kind is [ThemeKind::Custom]
   ///
   /// Use [Theme::from_custom()] instead
   pub fn new(kind: ThemeKind) -> Self {
      let theme = match kind {
         ThemeKind::Frappe => themes::frappe::theme(),
         ThemeKind::Latte => themes::latte::theme(),
         ThemeKind::TokyoNight => themes::tokyo_night::theme(),
         ThemeKind::Nord => themes::nord::theme(),
         ThemeKind::Custom => panic!("{}", PANIC_MSG),
      };

      theme
   }

   /// Load a custom theme from a json file
   pub fn from_custom(path: std::path::PathBuf) -> Result<Self, std::io::Error> {
      let data = std::fs::read(path)?;
      let mut theme: Theme = serde_json::from_slice::<Theme>(&data)?;
      theme.kind = ThemeKind::Custom;

      Ok(theme)
   }

   /// Serialize the theme to a json string
   pub fn to_json(&self) -> Result<String, serde_json::Error> {
      serde_json::to_string(self)
   }
}

/// These colors can be used to override the visuals using [egui::Ui::visuals_mut]
///
/// `border_color` = [egui::Stroke] color
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThemeColors {
   /// Main Background color for the entire app
   pub bg_color: Color32,

   /// Secondary Background color to make contrast with the main background
   ///
   /// Eg. Can be used as frame fill
   pub secondary_bg_color: Color32,

   /// Something to make good contrast with the main background
   pub extreme_bg_color: Color32,

   /// Background color for windows
   pub window_fill: Color32,

   /// Highlight something
   pub highlight1: Color32,

   pub highlight2: Color32,

   /// An overlay color, used for example when a window is open to darken the background
   pub overlay_color: Color32,

   /// Text color
   pub text_color: Color32,

   /// Secondary text color
   pub text_secondary: Color32,

   /// A color to use for errors (for most themes this could be a shade of red)
   ///
   /// This can also used to anything that a red color can be used for.
   pub error_color: Color32,

   /// A color to use for success (for most themes this could be a shade of green)
   ///
   /// This can also used to anything that a green color can be used for.
   pub success_color: Color32,

   /// A color to use for hyperlinks (for most themes this could be a shade of blue)
   pub hyperlink_color: Color32,

   /// Background color for text edits
   pub text_edit_bg: Color32,

   /// Background color for buttons
   pub button_bg: Color32,

   /// Widget background color
   pub widget_bg_color: Color32,

   /// Background color for active widgets (click)
   ///
   /// It affects the following widgets: Button, ComboxBox, Slider
   pub widget_bg_color_click: Color32,

   /// Background color for hovered widgets
   ///
   /// It affects the following widgets: Button, ComboxBox, Slider
   pub widget_bg_color_hover: Color32,

   /// Background color for open widgets (eg. a combo box)
   ///
   /// It affects the following widgets: ComboxBox
   pub widget_bg_color_open: Color32,

   /// Border color for inactive widgets (no hover or clicks)
   ///
   /// It affects the following widgets: Button, ComboxBox, TextEdit, Slider, RadioButton
   pub border_color_idle: Color32,

   /// Border color for active widgets (click)
   ///
   /// It affects the following widgets: Button, ComboxBox, TextEdit, Slider, RadioButton
   pub border_color_click: Color32,

   /// Border color for hovered widgets
   ///
   /// It affects the following widgets: Button, ComboxBox, TextEdit, Slider, RadioButton
   pub border_color_hover: Color32,

   /// Border color for open widgets (eg. an opened combo box)
   ///
   /// It affects the following widgets: ComboxBox
   pub border_color_open: Color32,
}

#[derive(Clone, Default, Debug, serde::Serialize, serde::Deserialize)]
pub struct TextSizes {
   pub very_small: f32,
   pub small: f32,
   pub normal: f32,
   pub large: f32,
   pub very_large: f32,
   pub heading: f32,
}

impl TextSizes {
   pub fn new(very_small: f32, small: f32, normal: f32, large: f32, very_large: f32, heading: f32) -> Self {
      Self {
         very_small,
         small,
         normal,
         large,
         very_large,
         heading,
      }
   }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FrameVisuals {
   pub bg_on_hover: Color32,
   pub bg_on_click: Color32,
   pub border_on_hover: (f32, Color32),
   pub border_on_click: (f32, Color32),
}

/// Visuals for ComboBoxes, Sliders
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct WidgetVisuals {
   pub bg_color_on_idle: Color32,
   pub bg_color_on_hover: Color32,
   pub bg_color_on_click: Color32,
   pub bg_color_on_open: Color32,
   pub combobox_bg: Color32,
   pub border_on_idle: (f32, Color32),
   pub border_on_hover: (f32, Color32),
   pub border_on_click: (f32, Color32),
   pub border_on_open: (f32, Color32),
}
