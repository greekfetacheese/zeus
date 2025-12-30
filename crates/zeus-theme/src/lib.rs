use egui::{Color32, Context, Frame, Id, LayerId, Order, Rect, Style};
use std::sync::{Arc, RwLock};

const PANIC_MSG: &str = "Custom theme not supported, use Theme::from_custom() instead";

pub mod editor;
pub mod hsla;
pub mod themes;
pub mod utils;
pub mod visuals;
pub mod window;

pub use editor::ThemeEditor;
use themes::*;
pub use visuals::*;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum ThemeKind {
   Dark,

   TokyoNight,

   /// WIP
  // Light,

   /// A custom theme
   Custom,
}

impl ThemeKind {
   pub fn to_str(&self) -> &str {
      match self {
         ThemeKind::Dark => "Dark",
         ThemeKind::TokyoNight => "Tokyo Night",
        // ThemeKind::Light => "Light",
         ThemeKind::Custom => "Custom",
      }
   }

   pub fn to_vec() -> Vec<Self> {
      vec![Self::Dark, Self::TokyoNight]
   }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct Theme {
   /// True if the theme is dark
   pub dark_mode: bool,
   #[cfg_attr(feature = "serde", serde(skip))]
   pub overlay_manager: OverlayManager,

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
         ThemeKind::TokyoNight => tokyo_night::theme(),
        // ThemeKind::Light => light::theme(),
         ThemeKind::Custom => panic!("{}", PANIC_MSG),
      };

      theme
   }

   pub fn set_window_frame_colors(&mut self) {
      match self.kind {
         ThemeKind::Dark => self.window_frame = dark::window_frame(&self.colors),
         ThemeKind::TokyoNight => self.window_frame = tokyo_night::window_frame(&self.colors),
        // ThemeKind::Light => self.window_frame = light::window_frame(&self.colors),
         ThemeKind::Custom => panic!("{}", PANIC_MSG),
      }
   }

   pub fn set_frame1_colors(&mut self) {
      match self.kind {
         ThemeKind::Dark => self.frame1 = dark::frame1(&self.colors),
         ThemeKind::TokyoNight => self.frame1 = tokyo_night::frame1(&self.colors),
        // ThemeKind::Light => self.frame1 = light::frame1(&self.colors),
         ThemeKind::Custom => panic!("{}", PANIC_MSG),
      }
   }

   pub fn set_frame2_colors(&mut self) {
      match self.kind {
         ThemeKind::Dark => self.frame2 = dark::frame2(&self.colors),
         ThemeKind::TokyoNight => self.frame2 = tokyo_night::frame2(&self.colors),
        // ThemeKind::Light => self.frame2 = light::frame2(&self.colors),
         ThemeKind::Custom => panic!("{}", PANIC_MSG),
      }
   }

   pub fn button_visuals(&self) -> ButtonVisuals {
      match self.kind {
         ThemeKind::Dark => self.colors.button_visuals,
         ThemeKind::TokyoNight => self.colors.button_visuals,
        // ThemeKind::Light => self.colors.button_visuals,
         ThemeKind::Custom => panic!("{}", PANIC_MSG),
      }
   }

   pub fn label_visuals(&self) -> LabelVisuals {
      match self.kind {
         ThemeKind::Dark => self.colors.label_visuals,
         ThemeKind::TokyoNight => self.colors.label_visuals,
        // ThemeKind::Light => self.colors.label_visuals,
         ThemeKind::Custom => panic!("{}", PANIC_MSG),
      }
   }

   pub fn combo_box_visuals(&self) -> ComboBoxVisuals {
      match self.kind {
         ThemeKind::Dark => self.colors.combo_box_visuals,
         ThemeKind::TokyoNight => self.colors.combo_box_visuals,
        // ThemeKind::Light => self.colors.combo_box_visuals,
         ThemeKind::Custom => panic!("{}", PANIC_MSG),
      }
   }

   pub fn text_edit_visuals(&self) -> TextEditVisuals {
      match self.kind {
         ThemeKind::Dark => self.colors.text_edit_visuals,
         ThemeKind::TokyoNight => self.colors.text_edit_visuals,
        // ThemeKind::Light => self.colors.text_edit_visuals,
         ThemeKind::Custom => panic!("{}", PANIC_MSG),
      }
   }
}

/// This is the color palette of the theme
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Copy, Clone, Debug)]
pub struct ThemeColors {
   pub button_visuals: ButtonVisuals,

   pub label_visuals: LabelVisuals,

   pub combo_box_visuals: ComboBoxVisuals,

   pub text_edit_visuals: TextEditVisuals,

   /// The color for the title bar of the app (if using custom window frame)
   pub title_bar: Color32,

   /// Main BG color of the theme
   pub bg: Color32,

   /// Widget BG color
   /// 
   /// This is the color of the widget backgrounds
   pub widget_bg: Color32,

   /// The color to use when hovering over a widget
   pub hover: Color32,

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

   /// Accent color
   pub accent: Color32,

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

#[derive(Clone, Debug, Default)]
pub struct OverlayManager(Arc<RwLock<OverlayCounter>>);

impl OverlayManager {
   pub fn new() -> Self {
      Self(Arc::new(RwLock::new(OverlayCounter::new())))
   }

   pub fn tint_0(&self) -> Color32 {
      Color32::from_black_alpha(40)
   }

   pub fn tint_1(&self) -> Color32 {
      Color32::from_black_alpha(60)
   }

   pub fn tint_2(&self) -> Color32 {
      Color32::from_black_alpha(80)
   }

   pub fn tint_3(&self) -> Color32 {
      Color32::from_black_alpha(100)
   }

   pub fn counter(&self) -> u8 {
      self.0.read().unwrap().counter()
   }

   pub fn order(&self) -> Order {
      self.0.read().unwrap().order()
   }

   pub fn paint_background(&self) {
      self.0.write().unwrap().paint_background()
   }

   pub fn paint_middle(&self) {
      self.0.write().unwrap().paint_middle()
   }

   pub fn paint_foreground(&self) {
      self.0.write().unwrap().paint_foreground()
   }

   pub fn paint_tooltip(&self) {
      self.0.write().unwrap().paint_tooltip()
   }

   pub fn paint_debug(&self) {
      self.0.write().unwrap().paint_debug()
   }

   /// Call this when you open a window
   pub fn window_opened(&self) {
      self.0.write().unwrap().window_opened();
   }

   /// Call this when you close a window
   pub fn window_closed(&self) {
      self.0.write().unwrap().window_closed();
   }

   pub fn recommended_order(&self) -> Order {
      self.0.read().unwrap().recommended_order()
   }

   pub fn calculate_alpha(&self) -> u8 {
      self.0.read().unwrap().calculate_alpha()
   }

   /// Returns the tint color based on the counter
   pub fn overlay_tint(&self) -> Color32 {
      self.0.read().unwrap().overlay_tint()
   }

   /// Paints a full-screen darkening overlay up to Foreground layer if needed
   ///
   /// If `recommend_order` is true, it will choose an order based on the counter
   pub fn paint_overlay(&self, ctx: &Context, recommend_order: bool) {
      self.0.read().unwrap().paint_overlay(ctx, recommend_order);
   }

   /// Paints an overlay at a specific screen position
   pub fn paint_overlay_at(&self, ctx: &Context, rect: Rect, order: Order, id: Id, tint: Color32) {
      self.0.read().unwrap().paint_overlay_at(ctx, rect, order, id, tint);
   }
}

#[derive(Clone, Debug)]
struct OverlayCounter {
   counter: u8,
   order: Order,
}

impl Default for OverlayCounter {
   fn default() -> Self {
      Self::new()
   }
}

impl OverlayCounter {
   pub fn new() -> Self {
      Self {
         counter: 0,
         order: Order::Background,
      }
   }

   pub fn counter(&self) -> u8 {
      self.counter
   }

   pub fn order(&self) -> Order {
      self.order
   }

   fn paint_background(&mut self) {
      self.order = Order::Background;
   }

   fn paint_middle(&mut self) {
      self.order = Order::Middle;
   }

   fn paint_foreground(&mut self) {
      self.order = Order::Foreground;
   }

   fn paint_tooltip(&mut self) {
      self.order = Order::Tooltip;
   }

   fn paint_debug(&mut self) {
      self.order = Order::Debug;
   }

   fn window_opened(&mut self) {
      self.counter += 1;
   }

   fn window_closed(&mut self) {
      if self.counter > 0 {
         self.counter -= 1;
      }
   }

   fn calculate_alpha(&self) -> u8 {
      let counter = self.counter;

      if counter == 0 {
         return 0;
      }

      let mut a = 60;
      for _ in 1..counter {
         a += 20;
      }

      a
   }

   fn overlay_tint(&self) -> Color32 {
      let counter = self.counter();

      if counter == 1 {
         return Color32::from_black_alpha(60);
      }

      let alpha = self.calculate_alpha();
      Color32::from_black_alpha(alpha)
   }

   fn recommended_order(&self) -> Order {
      if self.counter() == 1 {
         Order::Middle
      } else if self.counter() == 2 {
         Order::Foreground
      } else {
         Order::Tooltip
      }
   }

   fn paint_overlay(&self, ctx: &Context, recommend_order: bool) {
      let counter = self.counter();
      if counter == 0 {
         return;
      }

      let order = if recommend_order {
         if counter == 1 {
            Order::Middle
         } else if counter == 2 {
            Order::Foreground
         } else {
            Order::Tooltip
         }
      } else {
         self.order()
      };

      let layer_id = LayerId::new(order, Id::new("darkening_overlay"));

      let painter = ctx.layer_painter(layer_id);
      painter.rect_filled(ctx.content_rect(), 0.0, self.overlay_tint());
   }

   pub fn paint_overlay_at(&self, ctx: &Context, rect: Rect, order: Order, id: Id, tint: Color32) {
      let layer_id = LayerId::new(order, id);

      let painter = ctx.layer_painter(layer_id);
      painter.rect_filled(rect, 0.0, tint);
   }
}
