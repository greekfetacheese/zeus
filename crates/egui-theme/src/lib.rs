use egui::{Color32, Frame, style::Style};

const PANIC_MSG: &str = "Custom theme not supported, use Theme::from_custom() instead";

pub mod editor;
pub mod hsla;
pub mod themes;
pub mod utils;
pub mod window;

pub use editor::ThemeEditor;
use themes::*;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum ThemeKind {
   Dark,

   /// WIP
   Light,

   /// A custom theme
   Custom,
}

impl ThemeKind {
   pub fn to_str(&self) -> &str {
      match self {
         ThemeKind::Dark => "Dark",
         ThemeKind::Light => "Light",
         ThemeKind::Custom => "Custom",
      }
   }

   pub fn to_vec() -> Vec<Self> {
      vec![Self::Dark, Self::Light]
   }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct Theme {
   /// True if the theme is dark
   pub dark_mode: bool,

   /// True if a tint is recomended to be applied to images
   /// to soften the contrast between the image and the background
   /// 
   /// This is usually true for themes with very dark background
   pub image_tint_recommended: bool,
   pub kind: ThemeKind,
   pub style: Style,
   pub colors: ThemeColors,
   pub text_sizes: TextSizes,
   /// Used for [window::window_frame]
   pub window_frame: Frame,
   /// Base container frame for major UI sections.
   pub frame1: Frame,
   /// Frame for nested elements, like individual list items.
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
         ThemeKind::Dark => dark::theme(),
         ThemeKind::Light => light::theme(),
         ThemeKind::Custom => panic!("{}", PANIC_MSG),
      };

      theme
   }

   pub fn set_window_frame_colors(&mut self) {
      match self.kind {
         ThemeKind::Dark => self.window_frame = dark::window_frame(&self.colors),
         ThemeKind::Light => self.window_frame = light::window_frame(&self.colors),
         ThemeKind::Custom => panic!("{}", PANIC_MSG),
      }
   }

   pub fn set_frame1_colors(&mut self) {
      match self.kind {
         ThemeKind::Dark => self.frame1 = dark::frame1(&self.colors),
         ThemeKind::Light => self.frame1 = light::frame1(&self.colors),
         ThemeKind::Custom => panic!("{}", PANIC_MSG),
      }
   }

   pub fn set_frame2_colors(&mut self) {
      match self.kind {
         ThemeKind::Dark => self.frame2 = dark::frame2(&self.colors),
         ThemeKind::Light => self.frame2 = light::frame2(&self.colors),
         ThemeKind::Custom => panic!("{}", PANIC_MSG),
      }
   }
}


/// This is the color palette of the theme
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct ThemeColors {
   /// Main BG color of the theme
   ///
   /// This is usually the darkest color of the theme
   pub bg: Color32,

   /// BG2 color
   ///
   /// A secondary bg color that is applied on top of the `bg` color
   /// 
   /// Usually this is the color of choice for a popup window or a base frame/container
   /// 
   /// Its a should be a lighter shade/color of the `bg` color.
   pub bg2: Color32,

   /// BG3 color
   ///
   /// A third `bg` color that is applied on top of the `bg2` color
   /// 
   /// Usually this is the color of choice for a frame/container that is already inside a Ui that uses the `bg2` color
   /// 
   /// Its a should be a lighter shade/color of the `bg2` color.
   pub bg3: Color32,

   /// BG 4 color
   ///
   /// A fourth `bg` color that can be applied on top of the `bg3` color
   /// 
   /// Usually this is the color of choice for a frame/container that is already inside a Ui that uses the `bg3` color
   /// 
   /// Its a should be a lighter shade/color of the `bg3` color.
   pub bg4: Color32,

   /// Main text color
   pub text: Color32,

   /// Muted text color
   /// 
   /// For example a hint inside a text field
   pub text_muted: Color32,

   /// Highlight color
   pub highlight: Color32,

   /// Border color
   pub border: Color32,

   /// Primary action color
   pub primary: Color32,

   /// Secondary action color
   pub secondary: Color32,

   /// Error color
   /// 
   /// Can be used to indicate something bad or to highlight a dangerous action
   pub error: Color32,

   /// Warning color
   pub warning: Color32,

   /// Success color
   /// 
   /// Can be used to indicate something good or to highlight a successful action
   pub success: Color32,

   /// Info color
   ///
   /// Can be used for hyperlinks or to highlight something important
   pub info: Color32,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Default, Debug)]
pub struct TextSizes {
   pub very_small: f32,
   pub small: f32,
   pub normal: f32,
   pub large: f32,
   pub very_large: f32,
   pub heading: f32,
}

impl TextSizes {
   pub fn new(
      very_small: f32,
      small: f32,
      normal: f32,
      large: f32,
      very_large: f32,
      heading: f32,
   ) -> Self {
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

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug)]
pub struct FrameVisuals {
   pub bg_on_hover: Color32,
   pub bg_on_click: Color32,
   pub border_on_hover: (f32, Color32),
   pub border_on_click: (f32, Color32),
}
