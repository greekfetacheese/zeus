use egui::{Color32, Frame, style::Style};

const PANIC_MSG: &str = "Custom theme not supported, use Theme::from_custom() instead";

pub mod editor;
pub mod themes;
pub mod utils;

pub use editor::ThemeEditor;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ThemeKind {
   /// https://catppuccin.com/palette/
   Mocha,

   /// A custom theme
   Custom,
}

impl ThemeKind {
   pub fn to_str(&self) -> &str {
      match self {
         ThemeKind::Mocha => "Mocha",
         ThemeKind::Custom => "Custom",
      }
   }

   pub fn to_vec() -> Vec<Self> {
      vec![{ Self::Mocha }]
   }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Theme {
   pub kind: ThemeKind,
   pub style: Style,
   pub colors: ThemeColors,
   pub text_sizes: TextSizes,
   pub frame1: Frame,
   pub frame2: Frame,
   pub frame1_visuals: FrameVisuals,
   pub frame2_visuals: FrameVisuals,
}

impl Theme {
   /// Panics if the kind is [ThemeKind::Custom]
   ///
   /// Use [Theme::from_custom()] instead
   pub fn new(kind: ThemeKind) -> Self {
      let theme = match kind {
         ThemeKind::Mocha => themes::mocha::theme(),
         ThemeKind::Custom => panic!("{}", PANIC_MSG),
      };

      theme
   }

   /// Load a custom theme from a json file
   ///
   /// We expect the [Theme] to be serialized as it is
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

   /// Get the button visuals based on the bg color provided
   pub fn get_button_visuals(&self, bg_color: Color32) -> WidgetVisuals {
      match self.kind {
         ThemeKind::Mocha => themes::mocha::button_visuals(bg_color, &self.colors),
         ThemeKind::Custom => panic!("Not implemented"),
      }
   }

   /// Get the text edit visuals based on the bg color provided
   pub fn get_text_edit_visuals(&self, bg_color: Color32) -> WidgetVisuals {
      match self.kind {
         ThemeKind::Mocha => themes::mocha::text_edit_visuals(bg_color, &self.colors),
         ThemeKind::Custom => panic!("Not implemented"),
      }
   }

   /// Get the widget visuals based on the bg color provided
   pub fn get_widget_visuals(&self, bg_color: Color32) -> WidgetVisuals {
      match self.kind {
         ThemeKind::Mocha => themes::mocha::widget_visuals(bg_color, &self.colors),
         ThemeKind::Custom => panic!("Not implemented"),
      }
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

   /// Something to make good contrast with the `secondary_bg_color`
   pub extreme_bg_color2: Color32,

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

   /// Bg for TextEdit when the background is the main background
   pub text_edit_bg: Color32,

   /// Bg for TextEdit when the background is the `secondary_bg_color`
   pub text_edit_bg2: Color32,

   /// Bg for Buttons when the background is the main background
   pub button_bg: Color32,

   /// Bg for Buttons when the background is the `secondary_bg_color`
   pub button_bg2: Color32,

   /// Widget bg color when the background is the main background
   pub widget_bg_color: Color32,

   /// Widget bg color when the background is the `secondary_bg_color`
   pub widget_bg_color2: Color32,

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
