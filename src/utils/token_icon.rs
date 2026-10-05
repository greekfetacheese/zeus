use crate::assets::icons::save_token_icon;
use crate::core::urls::smoldapp_token_icon;
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use anyhow::anyhow;
use image::imageops::FilterType;
use std::io::Cursor;
use std::sync::OnceLock;
use std::time::Duration;
use zeus_eth::alloy_primitives::Address;

const MAX_ICON_BYTES: usize = 512 * 1024;
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

   if let Some(len) = response.content_length() {
      if len as usize > MAX_ICON_BYTES {
         return Err(anyhow!("icon too large ({len} bytes)"));
      }
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

   RT.spawn(async move {
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
}
