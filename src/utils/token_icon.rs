use crate::assets::icons::save_token_icon;
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use anyhow::anyhow;
use image::imageops::FilterType;
use std::io::Cursor;
use std::sync::OnceLock;
use std::time::Duration;
use zeus_eth::alloy_primitives::Address;

const SMOLDAPP_CDN: &str = "https://assets.smold.app/token";
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

fn smoldapp_url(chain_id: u64, address: Address) -> String {
   format!("{SMOLDAPP_CDN}/{chain_id}/{address:#x}/logo-32.png")
}

fn resize_to_png(data: &[u8], width: u32, height: u32) -> Result<Vec<u8>, anyhow::Error> {
   let image = image::load_from_memory(data)?;
   let resized = image.resize(width, height, FilterType::Lanczos3);
   let mut buf = Vec::new();
   resized.write_to(
      &mut Cursor::new(&mut buf),
      image::ImageFormat::Png,
   )?;
   Ok(buf)
}

/// Fetch the 32px SmolDapp icon and derive the 24px variant.
///
/// Returns `Ok(None)` on a 404 (token has no icon).
async fn fetch_smoldapp_icon(
   chain_id: u64,
   address: Address,
) -> Result<Option<(Vec<u8>, Vec<u8>)>, anyhow::Error> {
   let url = smoldapp_url(chain_id, address);
   let response = http_client().get(&url).send().await?;

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

   let bytes = response.bytes().await?;

   if bytes.is_empty() {
      return Err(anyhow!("empty icon response"));
   }

   if bytes.len() > MAX_ICON_BYTES {
      return Err(anyhow!("icon too large ({} bytes)", bytes.len()));
   }

   let x32 = bytes.to_vec();
   let x24 = resize_to_png(&x32, 24, 24)?;
   Ok(Some((x32, x24)))
}

/// Download the token icon from SmolDapp in the background.
///
/// Safe to call from any thread. Does not block on the network — missing
/// icons stay on the ERC-20 placeholder until the download finishes.
pub fn spawn_fetch_token_icon(chain_id: u64, address: Address) {
   let icons = SHARED_GUI.read(|gui| gui.icons.clone());
   if !icons.tokens.try_begin_fetch(address, chain_id) {
      return;
   }

   RT.spawn(async move {
      match fetch_smoldapp_icon(chain_id, address).await {
         Ok(Some((x32, x24))) => {
            if let Err(e) = save_token_icon(chain_id, address, &x32, &x24) {
               tracing::warn!("Failed to save token icon for {address} on chain {chain_id}: {e}");
            }

            icons.tokens.insert_icon(address, chain_id, x32, x24);
            icons.tokens.finish_fetch(address, chain_id, false);
            SHARED_GUI.write(|gui| {
               gui.request_repaint();
            });

            tracing::info!("Fetched token icon for {address} on chain {chain_id}");
         }
         Ok(None) => {
            tracing::debug!("No SmolDapp icon for {address} on chain {chain_id}");
            icons.tokens.finish_fetch(address, chain_id, true);
         }
         Err(e) => {
            tracing::warn!("Failed to fetch token icon for {address} on chain {chain_id}: {e}");
            icons.tokens.finish_fetch(address, chain_id, false);
         }
      }
   });
}
