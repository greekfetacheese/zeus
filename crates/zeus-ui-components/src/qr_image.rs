use egui::{Context, Image, ImageSource, load::Bytes};
use image::{ImageBuffer, ImageFormat, Luma};
use qrcodegen_no_heap::*;
use std::sync::Arc;

#[cfg(feature = "secure-types")]
use secure_types::Zeroize;

#[derive(Debug, Clone)]
pub enum QrError {
   EncodingFailed(String),
   ImageError(String),
   Other(String),
}

impl QrError {
   pub fn to_string(&self) -> String {
      match self {
         QrError::EncodingFailed(message) => format!("Encoding failed: {}", message),
         QrError::ImageError(message) => format!("Image failed: {}", message),
         QrError::Other(message) => message.to_string(),
      }
   }
}

impl std::fmt::Display for QrError {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      match self {
         QrError::EncodingFailed(message) => write!(f, "Encoding failed: {}", message),
         QrError::ImageError(message) => write!(f, "Image failed: {}", message),
         QrError::Other(message) => write!(f, "{}", message),
      }
   }
}

impl std::error::Error for QrError {}

/// Zeroizes the given image data when the `secure-types` feature is enabled.
///
/// Returns `true` only when the data was actually zeroized.
#[cfg(feature = "secure-types")]
fn zeroize_image_data(data: &mut [u8]) -> bool {
   data.zeroize();
   true
}

#[cfg(not(feature = "secure-types"))]
fn zeroize_image_data(_data: &mut [u8]) -> bool {
   false
}

/// A QR Code image.
///
/// The encoded image data lives in an `Arc<[u8]>` and uses the [`Bytes::Shared`] type
/// as the image source, which means no additional allocations are needed.
///
/// On `clear()` the image data is zeroized if the `secure-types` feature is enabled.
///
/// # Usage
///
/// ```
/// use egui::*;
/// use zeus_ui_components::QrImage;
///
/// struct MyUi {
///  open: bool,
///  qr: QrImage,
/// }
///
/// impl MyUi {
///
/// fn show(&mut self, ui: &mut Ui) {
/// if !self.open {
///  return;
/// }
///
/// let btn = Button::new("Show QR Code");
///
/// if ui.add(btn).clicked() {
///  let qr = QrImage::new("Hello, world!", "bytes://test".to_string());
///  self.qr = qr;
/// }
///
///  let image = self.qr.image();
///  ui.add(image);
///
/// // For the image to be cleared succesfully we need to call `clear()` on the `QrImage`
/// // when the image is out of scope (egui frame)
///
///  self.close_ui(ui);
/// }
///
/// fn close_ui(&mut self, ui: &mut Ui) {
///  let btn = Button::new("Close");
///  if ui.add(btn).clicked() {
///  // Close the UI so in the next frame the image doesn't show up
///  self.open = false;
///  self.qr.clear(ui.ctx());
///  }
/// }
/// }
/// ```
pub struct QrImage {
   /// Encoded QR image bytes (PNG), shared so egui can reference them cheaply.
   image_data: Arc<[u8]>,
   /// Cache key used by egui to identify and later forget the loaded image.
   uri: String,
   /// Stores a failure from encoding/rendering instead of ever erroring out.
   error: Option<QrError>,
}

impl QrImage {
   /// Create a new [QrImage] from the given data and uri.
   ///
   /// If the image encoding fails the error will be stored in the [QrImage].
   pub fn new(data: &str, uri: String) -> Self {
      let res = data_to_qr(data);
      let (image_data, error) = match res {
         Ok(image_data) => (image_data, None),
         Err(e) => (Vec::new(), Some(e)),
      };

      Self {
         image_data: image_data.into(),
         uri,
         error,
      }
   }

   /// Create an empty [QrImage] with an error.
   pub fn empty_with_error(error: String) -> Self {
      Self {
         image_data: Vec::new().into(),
         uri: String::new(),
         error: Some(QrError::Other(error)),
      }
   }

   pub fn has_error(&self) -> bool {
      self.error.is_some()
   }

   pub fn error(&self) -> Option<&QrError> {
      self.error.as_ref()
   }

   /// Returns true if the image is cleared
   pub fn is_cleared(&self) -> bool {
      self.image_data.is_empty()
   }

   /// Returns an [Image] that can be used in egui.
   ///
   /// It uses [Bytes::Shared] as the source so only one allocation exists.
   pub fn image(&self) -> Image<'static> {
      let data = self.image_data.clone();
      let image = Image::new(ImageSource::Bytes {
         uri: self.uri.clone().into(),
         bytes: Bytes::Shared(data),
      });
      image
   }

   /// Clears the QR Code image.
   ///
   /// If the `secure-types` feature is enabled, the image data will be zeroized.
   /// otherwise it will just be replaced with 0s which doesn't guarantee zeroization.
   ///
   /// # Returns
   /// `true` If the image was zeroized successfully.
   pub fn clear(&mut self, ctx: &Context) -> bool {
      ctx.forget_image(&self.uri);

      self.error = None;
      let success = match Arc::get_mut(&mut self.image_data) {
         Some(data) => {
            let did_zeroize = zeroize_image_data(data);
            self.image_data = Arc::new([0u8; 0]);
            did_zeroize
         }
         None => false,
      };

      success
   }
}

impl Drop for QrImage {
   fn drop(&mut self) {
      if let Some(data) = Arc::get_mut(&mut self.image_data) {
         zeroize_image_data(data);
      }
   }
}

fn data_to_qr(data: &str) -> Result<Vec<u8>, QrError> {
   let mut tempbuffer = vec![0u8; Version::MAX.buffer_len()];
   let mut outbuffer = vec![0u8; Version::MAX.buffer_len()];
   let qr = QrCode::encode_text(
      data,
      &mut tempbuffer,
      &mut outbuffer,
      QrCodeEcc::High,
      Version::MIN,
      Version::MAX,
      None,
      true,
   )
   .map_err(|e| QrError::EncodingFailed(e.to_string()))?;

   let size = qr.size() as u32;
   let module_px = ((512f32 / size as f32).ceil() as u32).max(1);
   let img_size = size * module_px;

   // Create white image buffer
   let mut img: ImageBuffer<Luma<u8>, Vec<u8>> = ImageBuffer::new(img_size, img_size);
   for pixel in img.pixels_mut() {
      *pixel = Luma([255u8]); // White background
   }

   for y in 0..size {
      for x in 0..size {
         if qr.get_module(x as i32, y as i32) {
            let px = Luma([0u8]); // Black
            for dy in 0..module_px {
               for dx in 0..module_px {
                  *img.get_pixel_mut(x * module_px + dx, y * module_px + dy) = px;
               }
            }
         }
      }
   }

   let mut png_bytes = Vec::new();
   img.write_to(&mut std::io::Cursor::new(&mut png_bytes), ImageFormat::Png)
      .map_err(|e| QrError::ImageError(e.to_string()))?;

   #[cfg(feature = "secure-types")]
   {
      let mut img_buf = img.into_vec();
      img_buf.zeroize();
      tempbuffer.zeroize();
      outbuffer.zeroize();
   }

   Ok(png_bytes)
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn test_qr_image() {
      let mut qr = QrImage::new("Hello, world!", "bytes://test".to_string());
      {
         let _image = qr.image();
      }

      assert!(!qr.is_cleared());
      assert!(!qr.has_error());

      qr.clear(&Context::default());
      assert!(qr.is_cleared());
   }
}
