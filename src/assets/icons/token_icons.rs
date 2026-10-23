use super::disk;
use super::load_image;
use crate::core::context::handle::RwLockHandle;
use eframe::egui::Context;
use eframe::egui::TextureHandle;
use eframe::egui::epaint::textures::TextureOptions;
use std::collections::HashMap;
use std::collections::HashSet;
use zeus_eth::ERC20Token;
use zeus_eth::alloy_primitives::Address;
use zeus_eth::types::ChainId;

pub struct TokenIcons {
   icons_x32: RwLockHandle<HashMap<(Address, u64), TextureHandle>>,
   /// Raw compressed 32×32 icon PNG bytes. Kept for lazy loading to avoid
   /// decompressing and uploading all textures at startup.
   icon_data: RwLockHandle<HashMap<(Address, u64), Vec<u8>>>,
   /// In-flight SmolDapp downloads so we don't spawn duplicates.
   in_flight: RwLockHandle<HashSet<(Address, u64)>>,
   /// 404s this session — don't retry until restart.
   failed: RwLockHandle<HashSet<(Address, u64)>>,
   egui_ctx: Context,
   pub erc20_x32: TextureHandle,
   pub bep20_x32: TextureHandle,
}

impl Default for TokenIcons {
   fn default() -> Self {
      let ctx = Context::default();
      let texture_options = TextureOptions::default();

      let erc20_x32 = load_image(include_bytes!("currency/resized/erc20.png")).unwrap();
      let bep20_x32 = load_image(include_bytes!("currency/resized/bep20.png")).unwrap();

      let erc20_x32 = ctx.load_texture("erc20_x32", erc20_x32, texture_options);
      let bep20_x32 = ctx.load_texture("bep20_x32", bep20_x32, texture_options);

      Self {
         icons_x32: RwLockHandle::new(HashMap::new()),
         icon_data: RwLockHandle::new(HashMap::new()),
         in_flight: RwLockHandle::new(HashSet::new()),
         failed: RwLockHandle::new(HashSet::new()),
         egui_ctx: ctx,
         erc20_x32,
         bep20_x32,
      }
   }
}

/// The baked 32×32 art of a base token, by symbol.
///
/// The default list no longer takes its icons from an external token blob, so the handful of tokens
/// it holds carry their art in the binary. `None` means no baked art: the token stays on the
/// ERC-20/BEP-20 placeholder until SmolDapp has an icon for it.
fn base_token_icon(symbol: &str) -> Option<&'static [u8]> {
   match symbol {
      "WETH" => Some(include_bytes!("currency/resized/weth.png")),
      "USDC" => Some(include_bytes!("currency/resized/usdc.png")),
      "USDT" => Some(include_bytes!("currency/resized/usdt.png")),
      "DAI" => Some(include_bytes!("currency/resized/dai.png")),
      "WBNB" => Some(include_bytes!("currency/resized/wbnb.png")),
      "USDG" => Some(include_bytes!("currency/resized/USDG.png")),
      _ => None,
   }
}

impl TokenIcons {
   pub fn new(ctx: &Context) -> Result<Self, anyhow::Error> {
      let mut icon_bytes: HashMap<(Address, u64), Vec<u8>> = HashMap::new();

      // Baked art for the default (base) token list
      for chain in ChainId::supported_chains() {
         for token in ERC20Token::base_tokens(chain.id()) {
            if let Some(png) = base_token_icon(&token.symbol) {
               icon_bytes.insert((token.address, token.chain_id), png.to_vec());
            }
         }
      }

      // Downloaded icons from previous sessions. Baked-in icons win.
      for (key, bytes) in disk::load_downloaded_icons() {
         icon_bytes.entry(key).or_insert(bytes);
      }

      let texture_options = TextureOptions::default();

      // ERC20 & BEP20 Placeholders - always loaded
      let erc20_x32 = load_image(include_bytes!("currency/resized/erc20.png"))?;
      let bep20_x32 = load_image(include_bytes!("currency/resized/bep20.png"))?;

      let erc20_x32 = ctx.load_texture("erc20_x32", erc20_x32, texture_options);
      let bep20_x32 = ctx.load_texture("bep20_x32", bep20_x32, texture_options);

      Ok(Self {
         icons_x32: RwLockHandle::new(HashMap::new()),
         icon_data: RwLockHandle::new(icon_bytes),
         in_flight: RwLockHandle::new(HashSet::new()),
         failed: RwLockHandle::new(HashSet::new()),
         egui_ctx: ctx.clone(),
         erc20_x32,
         bep20_x32,
      })
   }

   /// Get or lazily load the 32x32 texture for a token.
   pub(super) fn get_or_load_x32(&self, key: &(Address, u64)) -> Option<TextureHandle> {
      if let Some(handle) = self.icons_x32.read(|map| map.get(key).cloned()) {
         return Some(handle);
      }

      // Load from raw data (decompress + upload only when first used in UI)
      let data_x32 = self.icon_data.read(|icon_data| icon_data.get(key).cloned());

      if let Some(data_x32) = data_x32 {
         match load_image(&data_x32) {
            Ok(img) => {
               let name = format!("token32_{}", key.0);
               let handle = self.egui_ctx.load_texture(name, img, TextureOptions::default());
               self.icons_x32.write(|map| map.insert(*key, handle.clone()));
               return Some(handle);
            }
            Err(_e) => {
               #[cfg(feature = "dev")]
               tracing::warn!(
                  "Failed to decode token icon x32 for {}: {}",
                  key.0,
                  _e
               );
            }
         }
      }
      None
   }

   pub fn has_icon(&self, address: Address, chain_id: u64) -> bool {
      self.icon_data.read(|icon_data| icon_data.contains_key(&(address, chain_id)))
   }

   pub fn insert_icon(&self, address: Address, chain_id: u64, x32: Vec<u8>) {
      self.icon_data.write(|icon_data| icon_data.insert((address, chain_id), x32));
   }

   pub fn remove_icon(&self, address: Address, chain_id: u64) {
      let key = (address, chain_id);
      self.icon_data.write(|icon_data| icon_data.remove(&key));
      self.icons_x32.write(|icons_x32| icons_x32.remove(&key));
   }

   /// Mark a download as started. Returns false if we already have the icon,
   /// a fetch is in flight, or SmolDapp 404'd this session.
   pub fn try_begin_fetch(&self, address: Address, chain_id: u64) -> bool {
      let key = (address, chain_id);
      if self.has_icon(address, chain_id) {
         return false;
      }
      if self.failed.read(|failed| failed.contains(&key)) {
         return false;
      }
      self.in_flight.write(|in_flight| in_flight.insert(key))
   }

   pub fn finish_fetch(&self, address: Address, chain_id: u64, not_found: bool) {
      let key = (address, chain_id);
      self.in_flight.write(|in_flight| in_flight.remove(&key));
      if not_found {
         self.failed.write(|failed| failed.insert(key));
      }
   }
}
