use std::path::Path;

use tracing::{info, warn};
use zeus_eth::types::ChainId;

use crate::chains::assets_dir_for_chain;

/// Step 2: resize each remaining `logo.png` to 32×32 (Lanczos3) in place.
pub fn resize_icons(work_dir: &Path, chains: &[ChainId]) -> anyhow::Result<()> {
   for chain in chains {
      let asset_dir = assets_dir_for_chain(work_dir, *chain)?;
      if !asset_dir.exists() {
         warn!("Assets directory does not exist: {:?}", asset_dir);
         continue;
      }
      resize_icons_in_dir(&asset_dir)?;
   }

   info!("Icon data resized successfully");
   Ok(())
}

fn resize_icons_in_dir(directory: &Path) -> anyhow::Result<()> {
   for entry in std::fs::read_dir(directory)? {
      let entry = entry?;
      let path = entry.path();
      if !path.is_dir() {
         continue;
      }

      let logo_path = path.join("logo.png");
      let address = match path.file_name().and_then(|name| name.to_str()) {
         Some(name) => name.to_string(),
         None => {
            warn!("No file name for {:?}", path);
            continue;
         }
      };

      if logo_path.exists() && logo_path.is_file() {
         let img = match image::open(&logo_path) {
            Ok(img) => img,
            Err(e) => {
               warn!("Failed to open image for {address}: {e}");
               continue;
            }
         };

         let resized_img = img.resize(32, 32, image::imageops::FilterType::Lanczos3);
         if let Err(e) = resized_img.save(&logo_path) {
            warn!("Img save Error for {address}: {e}");
         }
      }
   }

   Ok(())
}

#[cfg(test)]
mod tests {
   use super::*;
   use image::{ImageBuffer, Rgba};
   use zeus_eth::types::ChainId;

   #[test]
   fn resizes_logo_to_32() {
      let tmp = tempfile::tempdir().unwrap();
      let token_dir =
         tmp.path().join("blockchains").join("ethereum").join("assets").join("0xtoken");
      std::fs::create_dir_all(&token_dir).unwrap();
      let logo = token_dir.join("logo.png");

      let img: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_pixel(64, 64, Rgba([1, 2, 3, 255]));
      img.save(&logo).unwrap();

      resize_icons(tmp.path(), &[ChainId::Ethereum]).unwrap();

      let resized = image::open(&logo).unwrap();
      assert_eq!(resized.width(), 32);
      assert_eq!(resized.height(), 32);
   }
}
