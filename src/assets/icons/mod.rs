#![allow(dead_code)]
#![allow(unused_variables)]

use eframe::egui::{
   ColorImage, Context, Image, ImageSource, Sense, TextureHandle, Vec2,
   epaint::textures::TextureOptions,
};
use std::borrow::Cow;

use crate::core::context::currencies::TokenData;
use crate::embedded::TOKEN_DATA;
use egui_elements::utils::TINT_1;
use image::imageops::FilterType;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::RwLock;
use zeus_eth::{alloy_primitives::Address, currency::Currency};

use bincode_next::{config::standard, decode_from_slice};

mod disk;
pub(crate) use disk::save_token_icon;

/// Icons used in the GUI
pub struct Icons {
   pub chain: ChainIcons,
   pub currency: CurrencyIcons,
   pub tokens: TokenIcons,
   pub misc: MiscIcons,
}

impl Default for Icons {
   fn default() -> Self {
      let egui_ctx = Context::default();
      let chain_icons = ChainIcons::new();
      let currency_icons = CurrencyIcons::new(&egui_ctx).unwrap();
      let misc_icons = MiscIcons::new(&egui_ctx).unwrap();

      Self {
         chain: chain_icons,
         currency: currency_icons,
         tokens: TokenIcons::default(),
         misc: misc_icons,
      }
   }
}

pub struct TokenIcons {
   icons_x32: RwLock<HashMap<(Address, u64), TextureHandle>>,
   icons_x24: RwLock<HashMap<(Address, u64), TextureHandle>>,
   /// Raw compressed icon PNG bytes. Kept for lazy loading to avoid
   /// decompressing and uploading all textures at startup.
   icon_data: RwLock<HashMap<(Address, u64), (Vec<u8>, Vec<u8>)>>,
   /// In-flight SmolDapp downloads so we don't spawn duplicates.
   in_flight: RwLock<HashSet<(Address, u64)>>,
   /// 404s this session — don't retry until restart.
   failed: RwLock<HashSet<(Address, u64)>>,
   egui_ctx: Context,
   pub erc20_x32: TextureHandle,
   pub erc20_x24: TextureHandle,
   pub bep20_x32: TextureHandle,
   pub bep20_x24: TextureHandle,
}

impl Default for TokenIcons {
   fn default() -> Self {
      let ctx = Context::default();
      let texture_options = TextureOptions::default();

      let erc20_x32 = load_image(include_bytes!("currency/resized/erc20.png")).unwrap();
      let bep20_x32 = load_image(include_bytes!("currency/resized/bep20.png")).unwrap();

      let erc20_x24 = load_image(include_bytes!("currency/resized/x24/erc20.png")).unwrap();
      let bep20_x24 = load_image(include_bytes!("currency/resized/x24/bep20.png")).unwrap();

      let erc20_x32 = ctx.load_texture("erc20_x32", erc20_x32, texture_options);
      let bep20_x32 = ctx.load_texture("bep20_x32", bep20_x32, texture_options);

      let erc20_x24 = ctx.load_texture("erc20_x24", erc20_x24, texture_options);
      let bep20_x24 = ctx.load_texture("bep20_x24", bep20_x24, texture_options);

      Self {
         icons_x32: RwLock::new(HashMap::new()),
         icons_x24: RwLock::new(HashMap::new()),
         icon_data: RwLock::new(HashMap::new()),
         in_flight: RwLock::new(HashSet::new()),
         failed: RwLock::new(HashSet::new()),
         egui_ctx: ctx,
         erc20_x32,
         bep20_x32,
         erc20_x24,
         bep20_x24,
      }
   }
}

impl TokenIcons {
   pub fn new(ctx: &Context) -> Result<Self, anyhow::Error> {
      let (icon_data, _bytes_read): (Vec<TokenData>, usize) =
         decode_from_slice(TOKEN_DATA, standard())?;

      #[cfg(feature = "dev")]
      tracing::info!("Loaded {} tokens", icon_data.len());

      let mut icon_bytes: HashMap<(Address, u64), (Vec<u8>, Vec<u8>)> = HashMap::new();

      for icon in icon_data {
         let address = Address::from_str(&icon.address)?;
         let key = (address, icon.chain_id);
         icon_bytes.insert(key, (icon.icon_data_x32, icon.icon_data_x24));
      }

      // Downloaded icons from previous sessions. Baked-in icons win.
      for (key, bytes) in disk::load_downloaded_icons() {
         icon_bytes.entry(key).or_insert(bytes);
      }

      let texture_options = TextureOptions::default();

      // ERC20 & BEP20 Placeholders - always loaded
      let erc20_x32 = load_image(include_bytes!("currency/resized/erc20.png"))?;
      let bep20_x32 = load_image(include_bytes!("currency/resized/bep20.png"))?;

      let erc20_x24 = load_image(include_bytes!("currency/resized/x24/erc20.png"))?;
      let bep20_x24 = load_image(include_bytes!("currency/resized/x24/bep20.png"))?;

      let erc20_x32 = ctx.load_texture("erc20_x32", erc20_x32, texture_options);
      let bep20_x32 = ctx.load_texture("bep20_x32", bep20_x32, texture_options);

      let erc20_x24 = ctx.load_texture("erc20_x24", erc20_x24, texture_options);
      let bep20_x24 = ctx.load_texture("bep20_x24", bep20_x24, texture_options);

      Ok(Self {
         icons_x32: RwLock::new(HashMap::new()),
         icons_x24: RwLock::new(HashMap::new()),
         icon_data: RwLock::new(icon_bytes),
         in_flight: RwLock::new(HashSet::new()),
         failed: RwLock::new(HashSet::new()),
         egui_ctx: ctx.clone(),
         erc20_x32,
         bep20_x32,
         erc20_x24,
         bep20_x24,
      })
   }

   /// Get or lazily load the 32x32 texture for a token.
   fn get_or_load_x32(&self, key: &(Address, u64)) -> Option<TextureHandle> {
      {
         let map = self.icons_x32.read().unwrap();
         if let Some(handle) = map.get(key) {
            return Some(handle.clone());
         }
      }

      // Load from raw data (decompress + upload only when first used in UI)
      let data_x32 = {
         let icon_data = self.icon_data.read().unwrap();
         icon_data.get(key).map(|(x32, _)| x32.clone())
      };

      if let Some(data_x32) = data_x32 {
         match load_image(&data_x32) {
            Ok(img) => {
               let name = format!("token32_{}", key.0);
               let handle = self.egui_ctx.load_texture(name, img, TextureOptions::default());
               let mut map = self.icons_x32.write().unwrap();
               map.insert(*key, handle.clone());
               return Some(handle);
            }
            Err(e) => {
               tracing::warn!(
                  "Failed to decode token icon x32 for {}: {}",
                  key.0,
                  e
               );
            }
         }
      }
      None
   }

   /// Get or lazily load the 24x24 texture for a token.
   fn get_or_load_x24(&self, key: &(Address, u64)) -> Option<TextureHandle> {
      {
         let map = self.icons_x24.read().unwrap();
         if let Some(handle) = map.get(key) {
            return Some(handle.clone());
         }
      }

      let data_x24 = {
         let icon_data = self.icon_data.read().unwrap();
         icon_data.get(key).map(|(_, x24)| x24.clone())
      };

      if let Some(data_x24) = data_x24 {
         match load_image(&data_x24) {
            Ok(img) => {
               let name = format!("token24_{}", key.0);
               let handle = self.egui_ctx.load_texture(name, img, TextureOptions::default());
               let mut map = self.icons_x24.write().unwrap();
               map.insert(*key, handle.clone());
               return Some(handle);
            }
            Err(e) => {
               tracing::warn!(
                  "Failed to decode token icon x24 for {}: {}",
                  key.0,
                  e
               );
            }
         }
      }
      None
   }

   pub fn has_icon(&self, address: Address, chain_id: u64) -> bool {
      self.icon_data.read().unwrap().contains_key(&(address, chain_id))
   }

   pub fn insert_icon(&self, address: Address, chain_id: u64, x32: Vec<u8>, x24: Vec<u8>) {
      self.icon_data.write().unwrap().insert((address, chain_id), (x32, x24));
   }

   /// Mark a download as started. Returns false if we already have the icon,
   /// a fetch is in flight, or SmolDapp 404'd this session.
   pub fn try_begin_fetch(&self, address: Address, chain_id: u64) -> bool {
      let key = (address, chain_id);
      if self.has_icon(address, chain_id) {
         return false;
      }
      if self.failed.read().unwrap().contains(&key) {
         return false;
      }
      let mut in_flight = self.in_flight.write().unwrap();
      in_flight.insert(key)
   }

   pub fn finish_fetch(&self, address: Address, chain_id: u64, not_found: bool) {
      let key = (address, chain_id);
      self.in_flight.write().unwrap().remove(&key);
      if not_found {
         self.failed.write().unwrap().insert(key);
      }
   }
}

pub struct ChainIcons {
   pub eth: ImageSource<'static>,
   pub op: ImageSource<'static>,
   pub bsc: ImageSource<'static>,
   pub base: ImageSource<'static>,
   pub arbitrum: ImageSource<'static>,
}

impl ChainIcons {
   pub fn new() -> Self {
      Self {
         eth: static_bytes_source(
            "bytes://chain/eth.png",
            include_bytes!("chain/eth.png"),
         ),
         op: static_bytes_source(
            "bytes://chain/op.svg",
            include_bytes!("chain/op.svg"),
         ),
         bsc: static_bytes_source(
            "bytes://chain/bsc.svg",
            include_bytes!("chain/bsc.svg"),
         ),
         base: static_bytes_source(
            "bytes://chain/base.svg",
            include_bytes!("chain/base.svg"),
         ),
         arbitrum: static_bytes_source(
            "bytes://chain/arbitrum.svg",
            include_bytes!("chain/arbitrum.svg"),
         ),
      }
   }
}

pub struct CurrencyIcons {
   pub eth: TextureHandle,
   pub eth_black: TextureHandle,
   pub eth_black_x24: TextureHandle,
   pub eth_x24: TextureHandle,
   pub bnb: TextureHandle,
   pub bnb_x24: TextureHandle,
}

impl CurrencyIcons {
   pub fn new(ctx: &Context) -> Result<Self, anyhow::Error> {
      let texture_options = TextureOptions::default();

      let eth_coin = load_image(include_bytes!("currency/resized/ethereum.png"))?;
      let eth_coin_x24 = load_image(include_bytes!(
         "currency/resized/x24/ethereum.png"
      ))?;

      let eth_black = load_image(include_bytes!("currency/resized/eth-black.png"))?;
      let eth_black_x24 = load_image(include_bytes!(
         "currency/resized/x24/eth-black.png"
      ))?;

      let bnb_coin = load_image(include_bytes!("currency/resized/bnb.png"))?;
      let bnb_coin_x24 = load_image(include_bytes!("currency/resized/x24/bnb.png"))?;

      Ok(Self {
         eth: ctx.load_texture("eth_coin", eth_coin, texture_options),
         eth_black: ctx.load_texture("eth_coin_black", eth_black, texture_options),
         eth_black_x24: ctx.load_texture(
            "eth_coin_black_x24",
            eth_black_x24,
            texture_options,
         ),
         eth_x24: ctx.load_texture("eth_coin_x24", eth_coin_x24, texture_options),
         bnb: ctx.load_texture("bnb_coin", bnb_coin, texture_options),
         bnb_x24: ctx.load_texture("bnb_coin_x24", bnb_coin_x24, texture_options),
      })
   }
}

pub struct MiscIcons {
   pub wallet_main_x24: TextureHandle,
}

impl MiscIcons {
   pub fn new(ctx: &Context) -> Result<Self, anyhow::Error> {
      let texture_options = TextureOptions::default();

      let wallet_main_x24 = load_image(include_bytes!("misc/x24/wallet-main.png"))?;

      Ok(Self {
         wallet_main_x24: ctx.load_texture(
            "wallet_main_x24",
            wallet_main_x24,
            texture_options,
         ),
      })
   }
}

impl Icons {
   pub fn new(ctx: &Context) -> Result<Self, anyhow::Error> {
      let texture_options = TextureOptions::default();

      let chain_icons = ChainIcons::new();
      let currency_icons = CurrencyIcons::new(ctx)?;
      let misc_icons = MiscIcons::new(ctx)?;

      Ok(Self {
         chain: chain_icons,
         currency: currency_icons,
         tokens: TokenIcons::new(ctx)?,
         misc: misc_icons,
      })
   }

   /// Return the chain icon based on the chain_id
   pub fn chain_icon(&self, id: u64, tint: bool) -> Image<'static> {
      let source = match id {
         1 => self.chain.eth.clone(),
         10 => self.chain.op.clone(),
         56 => self.chain.bsc.clone(),
         8453 => self.chain.base.clone(),
         42161 => self.chain.arbitrum.clone(),
         _ => self.chain.eth.clone(),
      };

      let mut img = Image::new(source)
         .fit_to_exact_size(Vec2::splat(24.0))
         .show_loading_spinner(false);

      if tint {
         img = img.tint(TINT_1);
      }

      img
   }

   pub fn native_currency_icon(&self, chain: u64, tint: bool) -> Image<'static> {
      let mut img = match chain {
         56 => Image::new(&self.currency.bnb),
         _ => Image::new(&self.currency.eth),
      };

      if tint {
         img = img.tint(TINT_1);
      }

      img
   }

   pub fn native_currency_icon_x24(&self, chain: u64, tint: bool) -> Image<'static> {
      let mut img = match chain {
         56 => Image::new(&self.currency.bnb_x24),
         _ => Image::new(&self.currency.eth_x24),
      };

      if tint {
         img = img.tint(TINT_1);
      }

      img
   }

   /// Return the currency icon based on the currency
   ///
   /// If the currency is native, it will return the native currency icon based on the chain_id
   ///
   /// If its ERC20, it will return the token icon based on the token address and chain id
   pub fn currency_icon_x32(&self, currency: &Currency, tint: bool) -> Image<'static> {
      if currency.is_native() {
         self.native_currency_icon(currency.chain_id(), tint)
      } else {
         self.token_icon_x32(currency.address(), currency.chain_id(), tint)
      }
   }

   pub fn currency_icon_x24(&self, currency: &Currency, tint: bool) -> Image<'static> {
      if currency.is_native() {
         self.native_currency_icon_x24(currency.chain_id(), tint)
      } else {
         self.token_icon_x24(currency.address(), currency.chain_id(), tint)
      }
   }

   /// Return the token icon (32 x 32) based on its address and chain id
   ///
   /// If it does not exist we return a placeholder.
   /// The texture is loaded lazily on first use to keep startup memory low.
   pub fn token_icon_x32(&self, address: Address, chain_id: u64, tint: bool) -> Image<'static> {
      let key = &(address, chain_id);
      if let Some(icon) = self.tokens.get_or_load_x32(key) {
         match tint {
            true => Image::new(&icon).tint(TINT_1),
            false => Image::new(&icon),
         }
      } else {
         self.token_placeholder_x32(chain_id, tint)
      }
   }

   pub fn token_icon_x24(&self, address: Address, chain_id: u64, tint: bool) -> Image<'static> {
      let key = &(address, chain_id);
      if let Some(icon) = self.tokens.get_or_load_x24(key) {
         match tint {
            true => Image::new(&icon).tint(TINT_1),
            false => Image::new(&icon),
         }
      } else {
         self.token_placeholder_x24(chain_id, tint)
      }
   }

   /// Return a placeholder icon for a token
   pub fn token_placeholder_x32(&self, id: u64, tint: bool) -> Image<'static> {
      let mut img = match id {
         56 => Image::new(&self.tokens.bep20_x32),
         _ => Image::new(&self.tokens.erc20_x32),
      };

      if tint {
         img = img.tint(TINT_1);
      }

      img
   }

   pub fn token_placeholder_x24(&self, id: u64, tint: bool) -> Image<'static> {
      let mut img = match id {
         56 => Image::new(&self.tokens.bep20_x24),
         _ => Image::new(&self.tokens.erc20_x24),
      };

      if tint {
         img = img.tint(TINT_1);
      }

      img
   }

   pub fn wallet_main_x24(&self) -> Image<'static> {
      Image::new(&self.misc.wallet_main_x24).sense(Sense::click())
   }
}

fn load_and_resize_image(
   image_data: &[u8],
   width: u32,
   height: u32,
) -> Result<ColorImage, image::ImageError> {
   let image = image::load_from_memory(image_data)?;
   let resized_image = image.resize(width, height, FilterType::Lanczos3);
   let size = [resized_image.width() as _, resized_image.height() as _];
   let image_buffer = resized_image.to_rgba8();
   let pixels = image_buffer.as_flat_samples();
   Ok(ColorImage::from_rgba_unmultiplied(
      size,
      pixels.as_slice(),
   ))
}

fn static_bytes_source(uri: &'static str, bytes: &'static [u8]) -> ImageSource<'static> {
   ImageSource::Bytes {
      uri: Cow::Borrowed(uri),
      bytes: bytes.into(),
   }
}

fn load_image(image_data: &[u8]) -> Result<ColorImage, image::ImageError> {
   let image = image::load_from_memory(image_data)?;
   let size = [image.width() as _, image.height() as _];
   let image_buffer = image.to_rgba8();
   let pixels = image_buffer.as_flat_samples();
   Ok(ColorImage::from_rgba_unmultiplied(
      size,
      pixels.as_slice(),
   ))
}
