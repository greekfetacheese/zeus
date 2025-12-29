use egui::Color32;
use palette::{Hsl, IntoColor, Srgba};

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy)]
pub struct Hsla {
   /// Hue 0.0..=360.0
   pub h: f32,
   /// Saturation 0.0..=100.0
   pub s: f32,
   /// Lightness 0.0..=100.0
   pub l: f32,
   /// Alpha 0.0..=1.0
   pub a: f32,
}

impl Hsla {
   pub fn from_color32(c: Color32) -> Self {
      let srgba = Srgba::new(
         c.r() as f32 / 255.0,
         c.g() as f32 / 255.0,
         c.b() as f32 / 255.0,
         c.a() as f32 / 255.0,
      );
      let hsl: Hsl = srgba.into_color();
      let (h, s, l) = hsl.into_components();
      // Normalize hue to [0, 360)
      let mut hue = h.into_degrees();
      hue = (hue % 360.0 + 360.0) % 360.0;
      Hsla {
         h: hue,
         s: s * 100.0,
         l: l * 100.0,
         a: srgba.alpha,
      }
   }

   pub fn from_hex(hex: &str) -> Option<Self> {
      match Color32::from_hex(hex) {
         Ok(c) => Some(Self::from_color32(c)),
         Err(_) => None,
      }
   }


   pub fn to_color32(&self) -> Color32 {
      let srgba = self.to_srgba();
      let (r, g, b, a) = srgba.into_components();
      Color32::from_rgba_unmultiplied(
         (r * 255.0) as u8,
         (g * 255.0) as u8,
         (b * 255.0) as u8,
         (a * 255.0) as u8,
      )
   }

   pub fn to_srgba(&self) -> Srgba {
      let hsl = Hsl::new(self.h, self.s / 100.0, self.l / 100.0);
      hsl.into_color()
   }

   pub fn to_rgba_components(&self) -> (u8, u8, u8, u8) {
      let srgba = self.to_srgba();
      let (r, g, b, a) = srgba.into_components();
      let r = (r * 255.0) as u8;
      let g = (g * 255.0) as u8;
      let b = (b * 255.0) as u8;
      let a = (a * 255.0) as u8;
      (r, g, b, a)
   }

   pub fn shades(&self, num_shades: usize, direction: ShadeDirection) -> Vec<Color32> {
      let mut shades = Vec::new();
      let step = if direction == ShadeDirection::Lighter {
         5.0
      } else {
         -5.0
      };
      let mut current = *self;
      for _ in 0..num_shades {
         shades.push(current.to_color32());
         current.l = (current.l as f32 + step).clamp(0.0, 100.0);
      }
      shades
   }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ShadeDirection {
   Lighter,
   Darker,
}