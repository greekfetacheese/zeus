use crate::assets::icons::{delete_token_icon, save_token_icon};
use crate::core::urls::smoldapp_token_icon;
use crate::gui::SHARED_GUI;
use anyhow::anyhow;
use egui_elements::{ChipRole, Toast};
use image::imageops::FilterType;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;
use zeus_eth::alloy_primitives::Address;

const MAX_ICON_BYTES: usize = 512 * 1024;

/// Cap for a picture the user picked, looser than `MAX_ICON_BYTES` because that file was chosen on
/// purpose and only stops a mis-picked video or huge photo being read at all. The stored art fits
/// a 32×32 box either way, and the file is read once.
const MAX_IMPORT_BYTES: usize = 2 * 1024 * 1024;

const FETCH_TIMEOUT: Duration = Duration::from_secs(15);

fn http_client() -> &'static reqwest::Client {
   static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
   CLIENT.get_or_init(|| {
      reqwest::Client::builder()
         .user_agent("zeus-wallet")
         .timeout(FETCH_TIMEOUT)
         .build()
         .unwrap_or_else(|_| reqwest::Client::new())
   })
}

fn resize_if_needed(data: &[u8], width: u32, height: u32) -> Result<Vec<u8>, anyhow::Error> {
   let image = image::load_from_memory(data)?;
   let is_x32 = image.width() == 32 && image.height() == 32;

   if is_x32 {
      return Ok(data.to_vec());
   }

   let resized = image.resize(width, height, FilterType::Lanczos3);
   let mut buf = Vec::new();
   resized.write_to(
      &mut Cursor::new(&mut buf),
      image::ImageFormat::Png,
   )?;
   Ok(buf)
}

/// Fetch the 32px SmolDapp icon.
///
/// Returns `Ok(None)` on a 404 (token has no icon).
async fn fetch_smoldapp_icon(
   chain_id: u64,
   address: Address,
) -> Result<Option<Vec<u8>>, anyhow::Error> {
   let url = smoldapp_token_icon(chain_id, address);
   let mut response = http_client().get(&url).send().await?;

   if response.status() == reqwest::StatusCode::NOT_FOUND {
      return Ok(None);
   }

   if !response.status().is_success() {
      return Err(anyhow!("SmolDapp returned {}", response.status()));
   }

   if let Some(len) = response.content_length()
      && len as usize > MAX_ICON_BYTES
   {
      return Err(anyhow!("icon too large ({len} bytes)"));
   }

   // Streamed rather than buffered: `Response::bytes` would take whatever the host sends before the
   // cap below could run — the timeout alone at line rate is megabytes per fetch (see
   // [`read_icon_body`]).
   let Some(bytes) = read_icon_body(&mut response, &url).await? else {
      return Err(anyhow!("empty icon response"));
   };

   let icon = resize_if_needed(&bytes, 32, 32)?;
   Ok(Some(icon))
}

/// Read an icon body chunk by chunk, refusing to hold more than [`MAX_ICON_BYTES`].
///
/// The declared length is a hint, not the cap: a chunked response declares none, and a hostile one can
/// lie. `Ok(None)` is an empty body, which the caller reports as a missing icon.
async fn read_icon_body(
   response: &mut reqwest::Response,
   url: &str,
) -> Result<Option<Vec<u8>>, anyhow::Error> {
   let mut body = Vec::new();

   while let Some(chunk) = response.chunk().await? {
      if body.len() + chunk.len() > MAX_ICON_BYTES {
         return Err(anyhow!(
            "{url} is larger than {MAX_ICON_BYTES} bytes"
         ));
      }

      body.extend_from_slice(&chunk);
   }

   Ok((!body.is_empty()).then_some(body))
}

/// Normalise a picture the user picked into the stored `x32.png`.
///
/// Scaled to fit a 32×32 box with the aspect ratio kept, the same way a downloaded icon is, so a
/// wide logo is not squashed. Unlike the fetcher this never passes the source through: the picker
/// takes gif/webp/jpeg but the file written is `x32.png`, so an import is re-encoded even when it
/// already fits.
fn icon_from_user_file(bytes: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
   if bytes.len() > MAX_IMPORT_BYTES {
      return Err(anyhow!(
         "image is {} bytes, the import cap is {MAX_IMPORT_BYTES} bytes",
         bytes.len()
      ));
   }

   let image = image::load_from_memory(bytes)?;
   let resized = image.resize(32, 32, FilterType::Lanczos3);
   let mut buf = Vec::new();
   resized.write_to(
      &mut Cursor::new(&mut buf),
      image::ImageFormat::Png,
   )?;
   Ok(buf)
}

/// Read, normalise and persist the picked picture.
///
/// The size check runs on the metadata before the read, so a huge file never reaches memory. The
/// result goes to the same path the fetcher writes, so the next start loads it like any icon.
fn import_icon_file(
   chain_id: u64,
   address: Address,
   path: &Path,
) -> Result<Vec<u8>, anyhow::Error> {
   let len = std::fs::metadata(path)?.len();
   if len as usize > MAX_IMPORT_BYTES {
      return Err(anyhow!(
         "{} is {len} bytes, the import cap is {MAX_IMPORT_BYTES} bytes",
         path.display()
      ));
   }

   let bytes = std::fs::read(path)?;
   let icon = icon_from_user_file(&bytes)?;
   save_token_icon(chain_id, address, &icon)?;
   Ok(icon)
}

/// Download the token icon from SmolDapp in the background.
///
/// Safe to call from any thread. Does not block on the network — missing
/// icons stay on the ERC-20 placeholder until the download finishes.
pub fn spawn_fetch_token_icon(chain_id: u64, address: Address) {
   let (icons, allowed) = SHARED_GUI.read(|gui| {
      (
         gui.icons.clone(),
         gui.ctx.read(|ctx| ctx.misc_config.fetch_asset_images()),
      )
   });
   if !allowed {
      return;
   }
   if !icons.tokens.try_begin_fetch(address, chain_id) {
      return;
   }

   crate::runtime::rt().spawn(async move {
      match fetch_smoldapp_icon(chain_id, address).await {
         Ok(Some(icon)) => {
            if let Err(_e) = save_token_icon(chain_id, address, &icon) {
               #[cfg(feature = "dev")]
               tracing::warn!("Failed to save token icon for {address} on chain {chain_id}: {_e}");
            }

            icons.tokens.insert_icon(address, chain_id, icon);
            icons.tokens.finish_fetch(address, chain_id, false);
            SHARED_GUI.write(|gui| {
               gui.request_repaint();
            });

            #[cfg(feature = "dev")]
            tracing::info!("Fetched token icon for {address} on chain {chain_id}");
         }
         Ok(None) => {
            #[cfg(feature = "dev")]
            tracing::debug!("No SmolDapp icon for {address} on chain {chain_id}");
            icons.tokens.finish_fetch(address, chain_id, true);
         }
         Err(_e) => {
            #[cfg(feature = "dev")]
            tracing::warn!("Failed to fetch token icon for {address} on chain {chain_id}: {_e}");
            icons.tokens.finish_fetch(address, chain_id, false);
         }
      }
   });
}

/// Replace the art of a token with a picture the user picked.
///
/// File I/O and decoding run on a blocking worker; the GUI lock is only taken to swap the texture
/// and toast. A replaced icon needs its old texture dropped first, which insert_icon alone misses.
pub fn spawn_import_token_icon(chain_id: u64, address: Address, symbol: String, path: PathBuf) {
   crate::runtime::rt().spawn_blocking(
      move || match import_icon_file(chain_id, address, &path) {
         Ok(icon) => {
            SHARED_GUI.write(|gui| {
               gui.icons.tokens.remove_icon(address, chain_id);
               gui.icons.tokens.insert_icon(address, chain_id, icon);
               Toast::new("Icon Imported")
                  .description(format!("{symbol} now uses the imported picture"))
                  .tone(ChipRole::Success)
                  .show(&gui.egui_ctx);
               gui.request_repaint();
            });
         }
         Err(e) => {
            SHARED_GUI.write(|gui| {
               Toast::new("Icon Import Failed")
                  .description(e.to_string())
                  .tone(ChipRole::Error)
                  .persistent()
                  .show(&gui.egui_ctx);
               gui.request_repaint();
            });
         }
      },
   );
}

/// Drop stored art and return to what the app shows with no user choice.
///
/// A base token gets its baked icon back; anything else is re-fetched from SmolDapp. The fetch takes
/// the GUI lock itself, so it runs outside the guard below. A failed delete is not worth a toast:
/// the in-memory reset still shows, and the next import overwrites the file.
pub fn spawn_reset_token_icon(chain_id: u64, address: Address, symbol: String) {
   crate::runtime::rt().spawn_blocking(move || {
      if let Err(_e) = delete_token_icon(chain_id, address) {
         #[cfg(feature = "dev")]
         tracing::warn!("Failed to delete token icon for {address} on chain {chain_id}: {_e}");
      }

      let restored = SHARED_GUI.write(|gui| {
         gui.icons.tokens.remove_icon(address, chain_id);
         gui.icons.tokens.restore_baked(address, chain_id, &symbol)
      });

      if !restored {
         spawn_fetch_token_icon(chain_id, address);
      }

      SHARED_GUI.write(|gui| {
         Toast::new("Icon Reset")
            .description(format!("{symbol} is back on its default icon"))
            .tone(ChipRole::Success)
            .show(&gui.egui_ctx);
         gui.request_repaint();
      });
   });
}

#[cfg(test)]
mod tests {
   use super::*;

   /// The cap has to bite *mid-stream*.
   ///
   /// With no `Content-Length` — chunked, or anything else a hostile host feels like sending — the
   /// only bound `Response::bytes` left was the timeout, so a fetch could hold megabytes before
   /// anything checked the size. The server here writes past the cap and then holds the connection
   /// open: nothing but the cap can end the read, so a buffering reader hangs until the clock fires.
   #[tokio::test]
   async fn an_oversized_icon_body_is_refused() {
      use tokio::io::AsyncWriteExt;

      let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
      let addr = listener.local_addr().unwrap();

      let server = tokio::spawn(async move {
         let (mut socket, _) = listener.accept().await.unwrap();
         let head =
            b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nTransfer-Encoding: chunked\r\n\r\n";
         socket.write_all(head).await.unwrap();

         // 640 KiB for a cap of 512, then the connection stays open.
         let chunk = vec![0xABu8; 32 * 1024];
         for _ in 0..20 {
            let header = format!("{:x}\r\n", chunk.len());
            let wrote = async {
               socket.write_all(header.as_bytes()).await?;
               socket.write_all(&chunk).await?;
               socket.write_all(b"\r\n").await
            };

            if wrote.await.is_err() {
               return;
            }
         }

         tokio::time::sleep(Duration::from_secs(60)).await;
      });

      let mut response = reqwest::Client::new()
         .get(format!("http://{addr}/logo-32.png"))
         .send()
         .await
         .unwrap();

      let read = tokio::time::timeout(
         Duration::from_secs(5),
         read_icon_body(&mut response, "http://large/logo-32.png"),
      )
      .await
      .expect("the cap has to end the read, not the clock");

      assert!(
         read.is_err(),
         "a body past the cap must be refused"
      );
      server.abort();
   }

   fn png_of(width: u32, height: u32) -> Vec<u8> {
      let image = image::RgbaImage::from_pixel(
         width,
         height,
         image::Rgba([0x33, 0x66, 0x99, 0xFF]),
      );
      let mut buf = Vec::new();
      image::DynamicImage::ImageRgba8(image)
         .write_to(
            &mut Cursor::new(&mut buf),
            image::ImageFormat::Png,
         )
         .unwrap();
      buf
   }

   /// The cap is a guard against a mis-picked file, so the refusal has to say both numbers.
   #[test]
   fn an_import_past_the_cap_is_refused() {
      let bytes = vec![0u8; MAX_IMPORT_BYTES + 1];
      let message = icon_from_user_file(&bytes).unwrap_err().to_string();

      assert!(
         message.contains(&bytes.len().to_string()),
         "{message}"
      );
      assert!(
         message.contains(&MAX_IMPORT_BYTES.to_string()),
         "{message}"
      );
   }

   #[test]
   fn an_import_is_scaled_to_x32_png() {
      let icon = icon_from_user_file(&png_of(64, 64)).unwrap();
      let decoded = image::load_from_memory(&icon).unwrap();

      assert_eq!(
         image::guess_format(&icon).unwrap(),
         image::ImageFormat::Png
      );
      assert_eq!((decoded.width(), decoded.height()), (32, 32));
   }

   /// The picker takes gif/webp/jpeg, but the file written is `x32.png`, so no passthrough.
   #[test]
   fn an_already_x32_gif_is_still_re_encoded() {
      let source = image::RgbaImage::from_pixel(32, 32, image::Rgba([0x10, 0x20, 0x30, 0xFF]));
      let mut gif = Vec::new();
      image::codecs::gif::GifEncoder::new(&mut gif)
         .encode(
            source.as_raw(),
            32,
            32,
            image::ExtendedColorType::Rgba8,
         )
         .unwrap();
      assert_eq!(
         image::guess_format(&gif).unwrap(),
         image::ImageFormat::Gif
      );

      let icon = icon_from_user_file(&gif).unwrap();

      assert_eq!(
         image::guess_format(&icon).unwrap(),
         image::ImageFormat::Png
      );
   }

   #[test]
   fn a_non_image_is_an_error_not_a_panic() {
      assert!(icon_from_user_file(b"definitely not a picture").is_err());
   }
}
