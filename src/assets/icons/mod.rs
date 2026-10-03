#![allow(dead_code)]
#![allow(unused_variables)]

use eframe::egui::{
   ColorImage, Context, Image, ImageSource, Sense, TextureHandle, Vec2,
   epaint::textures::TextureOptions,
};
use std::borrow::Cow;
use zeus_eth::{ERC20Token, types::ChainId};

use crate::core::context::currencies::TokenData;
use crate::embedded::TOKEN_DATA;
use egui_elements::utils::TINT_1;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::RwLock;
use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::Currency,
};

use bincode_next::{config::standard, decode_from_slice};

mod disk;
pub(crate) use disk::{delete_nft_icon, delete_token_icon, save_nft_icon, save_token_icon};

/// Icons used in the GUI
pub struct Icons {
   pub chain: ChainIcons,
   pub currency: CurrencyIcons,
   pub tokens: TokenIcons,
   pub nfts: NftIcons,
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
         nfts: NftIcons::default(),
         misc: misc_icons,
      }
   }
}

pub struct TokenIcons {
   icons_x32: RwLock<HashMap<(Address, u64), TextureHandle>>,
   /// Raw compressed 32×32 icon PNG bytes. Kept for lazy loading to avoid
   /// decompressing and uploading all textures at startup.
   icon_data: RwLock<HashMap<(Address, u64), Vec<u8>>>,
   /// In-flight SmolDapp downloads so we don't spawn duplicates.
   in_flight: RwLock<HashSet<(Address, u64)>>,
   /// 404s this session — don't retry until restart.
   failed: RwLock<HashSet<(Address, u64)>>,
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
         icons_x32: RwLock::new(HashMap::new()),
         icon_data: RwLock::new(HashMap::new()),
         in_flight: RwLock::new(HashSet::new()),
         failed: RwLock::new(HashSet::new()),
         egui_ctx: ctx,
         erc20_x32,
         bep20_x32,
      }
   }
}

impl TokenIcons {
   pub fn new(ctx: &Context) -> Result<Self, anyhow::Error> {
      let (icon_data, _bytes_read): (Vec<TokenData>, usize) =
         decode_from_slice(TOKEN_DATA, standard())?;

      #[cfg(feature = "dev")]
      tracing::info!("Loaded {} tokens", icon_data.len());

      let mut icon_bytes: HashMap<(Address, u64), Vec<u8>> = HashMap::new();

      for icon in icon_data {
         let address = Address::from_str(&icon.address)?;
         let key = (address, icon.chain_id);
         icon_bytes.insert(key, icon.icon_data_x32);
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

      // Robinhood USDG
      let usdg_x32 = include_bytes!("currency/resized/USDG.png");
      let usdg_token = ERC20Token::usdg_robinhood();

      // Robinhood WETH
      let weth_x32 = include_bytes!("currency/resized/weth.png");
      let weth_token = ERC20Token::weth_robinhood();

      icon_bytes.insert(
         (usdg_token.address, usdg_token.chain_id),
         usdg_x32.to_vec(),
      );
      icon_bytes.insert(
         (weth_token.address, weth_token.chain_id),
         weth_x32.to_vec(),
      );

      Ok(Self {
         icons_x32: RwLock::new(HashMap::new()),
         icon_data: RwLock::new(icon_bytes),
         in_flight: RwLock::new(HashSet::new()),
         failed: RwLock::new(HashSet::new()),
         egui_ctx: ctx.clone(),
         erc20_x32,
         bep20_x32,
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
         icon_data.get(key).cloned()
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

   pub fn has_icon(&self, address: Address, chain_id: u64) -> bool {
      self.icon_data.read().unwrap().contains_key(&(address, chain_id))
   }

   pub fn insert_icon(&self, address: Address, chain_id: u64, x32: Vec<u8>) {
      self.icon_data.write().unwrap().insert((address, chain_id), x32);
   }

   pub fn remove_icon(&self, address: Address, chain_id: u64) {
      let key = (address, chain_id);
      self.icon_data.write().unwrap().remove(&key);
      self.icons_x32.write().unwrap().remove(&key);
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

/// Key for one NFT's images: `(collection, chain id, token id)`.
pub type NftKey = (Address, u64, U256);

/// Stored art for one NFT.
///
/// Two forms because the two kinds of art want opposite treatment: raster art is pre-rendered
/// (a grid and a detail view ask for different sizes, and decoding once beats decoding per view),
/// while vector art is kept as its source because egui rasterises SVG at whatever size it is asked
/// for — pre-rendering it would only lose quality and cost disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NftIconData {
   /// Pre-rendered raster art: a list thumbnail and a detail-view copy.
   Raster { x64: Vec<u8>, x250: Vec<u8> },
   /// The SVG source, verbatim.
   Svg(Vec<u8>),
}

impl NftIconData {
   /// Whether this holds anything worth showing — a half-written directory holds nothing.
   pub fn is_empty(&self) -> bool {
      match self {
         Self::Raster { x64, x250 } => x64.is_empty() && x250.is_empty(),
         Self::Svg(svg) => svg.is_empty(),
      }
   }
}

/// Downloaded NFT images.
///
/// Keyed by collection **and** token id, because one collection holds many tokens — an address
/// alone would give every token in a collection the first token's picture. Unlike token icons there
/// is no baked-in set: every NFT image comes from the chain or not at all.
pub struct NftIcons {
   icons_x64: RwLock<HashMap<NftKey, TextureHandle>>,
   icons_x250: RwLock<HashMap<NftKey, TextureHandle>>,
   /// Raw PNG bytes. Kept so textures are decompressed and uploaded only when a view asks.
   icon_data: RwLock<HashMap<NftKey, NftIconData>>,
   /// In-flight downloads, so two views asking at once do not fetch twice.
   in_flight: RwLock<HashSet<NftKey>>,
   /// Tokens whose image was missing this session — don't retry until restart.
   failed: RwLock<HashSet<NftKey>>,
   egui_ctx: Context,
   /// Shown when a token has no image, or its download has not finished yet.
   pub placeholder: TextureHandle,
}

impl Default for NftIcons {
   fn default() -> Self {
      let egui_ctx = Context::default();
      Self::new(&egui_ctx).unwrap()
   }
}

impl NftIcons {
   pub fn new(ctx: &Context) -> Result<Self, anyhow::Error> {
      let placeholder = load_image(include_bytes!("nft/placeholder.png"))?;
      let placeholder = ctx.load_texture(
         "nft_placeholder",
         placeholder,
         TextureOptions::default(),
      );

      // Images downloaded in previous sessions.
      let icon_data: HashMap<NftKey, NftIconData> = disk::load_downloaded_nft_icons();

      Ok(Self {
         icons_x64: RwLock::new(HashMap::new()),
         icons_x250: RwLock::new(HashMap::new()),
         icon_data: RwLock::new(icon_data),
         in_flight: RwLock::new(HashSet::new()),
         failed: RwLock::new(HashSet::new()),
         egui_ctx: ctx.clone(),
         placeholder,
      })
   }

   /// The raster texture for one rendering, if the stored art is raster.
   ///
   /// Falls back to the other rendering rather than to the placeholder: a 250px copy shown in a
   /// 64px row is still the right picture.
   fn raster_texture(&self, key: &NftKey, large: bool) -> Option<TextureHandle> {
      let cache = if large {
         &self.icons_x250
      } else {
         &self.icons_x64
      };

      if let Some(handle) = cache.read().unwrap().get(key) {
         return Some(handle.clone());
      }

      let bytes = {
         let icon_data = self.icon_data.read().unwrap();
         // Vector art has no raster renderings to build a texture from.
         let NftIconData::Raster { x64, x250 } = icon_data.get(key)? else {
            return None;
         };
         let (preferred, other) = if large { (x250, x64) } else { (x64, x250) };
         if preferred.is_empty() {
            other.clone()
         } else {
            preferred.clone()
         }
      };

      if bytes.is_empty() {
         return None;
      }

      match load_image(&bytes) {
         Ok(image) => {
            let size = if large { 250 } else { 64 };
            let name = format!("nft{}_{}_{}_{}", size, key.0, key.1, key.2);
            let handle = self.egui_ctx.load_texture(name, image, TextureOptions::default());
            cache.write().unwrap().insert(*key, handle.clone());
            Some(handle)
         }
         Err(e) => {
            tracing::warn!("Failed to decode NFT image for {}: {}", key.0, e);
            None
         }
      }
   }

   pub fn has_icon(&self, key: &NftKey) -> bool {
      self.icon_data.read().unwrap().get(key).is_some_and(|data| !data.is_empty())
   }

   /// A token id of `collection` that has art cached, if any. Lowest id wins, so the choice is stable
   /// across frames rather than whatever the map happens to iterate first.
   ///
   /// For rows that are about a collection rather than one token — an approval log carries no token id —
   /// where asking for a fixed id (0, say) can land on a token whose art was never fetched.
   fn cached_id_for_collection(&self, chain_id: u64, collection: Address) -> Option<U256> {
      self
         .icon_data
         .read()
         .unwrap()
         .iter()
         .filter(|(key, data)| key.0 == collection && key.1 == chain_id && !data.is_empty())
         .map(|(key, _)| key.2)
         .min()
   }

   /// Whether this token still needs a download attempt.
   ///
   /// A read-only probe of the same three conditions [`NftIcons::try_begin_fetch`] enforces, for
   /// callers that have to *count* the fetches they start: a list that downloads art for its first N
   /// rows per load must skip the ones already tried, or the tail of a long list is never reached.
   pub fn needs_fetch(&self, key: &NftKey) -> bool {
      !self.has_icon(key)
         && !self.in_flight.read().unwrap().contains(key)
         && !self.failed.read().unwrap().contains(key)
   }

   /// The vector source for this token, if the stored art is an SVG.
   ///
   /// egui rasterises it at the size a view asks for, so there is nothing to cache as a texture.
   fn svg_source(&self, key: &NftKey) -> Option<Vec<u8>> {
      match self.icon_data.read().unwrap().get(key)? {
         NftIconData::Svg(svg) => Some(svg.clone()),
         NftIconData::Raster { .. } => None,
      }
   }

   pub fn insert_icon(&self, key: NftKey, data: NftIconData) {
      self.icon_data.write().unwrap().insert(key, data);
   }

   pub fn remove_icon(&self, key: &NftKey) {
      self.icon_data.write().unwrap().remove(key);
      self.icons_x64.write().unwrap().remove(key);
      self.icons_x250.write().unwrap().remove(key);
   }

   /// Mark a download as started. Returns false when we already have the image, one is in flight,
   /// or it came back missing this session.
   pub fn try_begin_fetch(&self, key: &NftKey) -> bool {
      if self.has_icon(key) {
         return false;
      }
      if self.failed.read().unwrap().contains(key) {
         return false;
      }
      self.in_flight.write().unwrap().insert(*key)
   }

   pub fn finish_fetch(&self, key: &NftKey, not_found: bool) {
      self.in_flight.write().unwrap().remove(key);
      if not_found {
         self.failed.write().unwrap().insert(*key);
      }
   }
}

pub struct ChainIcons {
   pub eth: ImageSource<'static>,
   pub op: ImageSource<'static>,
   pub bsc: ImageSource<'static>,
   pub base: ImageSource<'static>,
   pub arbitrum: ImageSource<'static>,
   pub robinhood: ImageSource<'static>,
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
         robinhood: static_bytes_source(
            "bytes://chain/robinhood.png",
            include_bytes!("chain/robinhood.png"),
         ),
      }
   }

   pub fn for_chain(&self, chain: ChainId) -> ImageSource<'static> {
      match chain {
         ChainId::Ethereum | ChainId::EthereumSepolia => self.eth.clone(),
         ChainId::Optimism => self.op.clone(),
         ChainId::BinanceSmartChain => self.bsc.clone(),
         ChainId::Base => self.base.clone(),
         ChainId::Arbitrum => self.arbitrum.clone(),
         ChainId::RobinHood => self.robinhood.clone(),
      }
   }
}

pub struct CurrencyIcons {
   pub eth: TextureHandle,
   pub bnb: TextureHandle,
}

impl CurrencyIcons {
   pub fn new(ctx: &Context) -> Result<Self, anyhow::Error> {
      let texture_options = TextureOptions::default();

      let eth_coin = load_image(include_bytes!("currency/resized/ethereum.png"))?;
      let bnb_coin = load_image(include_bytes!("currency/resized/bnb.png"))?;

      Ok(Self {
         eth: ctx.load_texture("eth_coin", eth_coin, texture_options),
         bnb: ctx.load_texture("bnb_coin", bnb_coin, texture_options),
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
         nfts: NftIcons::new(ctx)?,
         misc: misc_icons,
      })
   }

   /// Return the chain icon based on the chain_id
   ///
   /// Unsupported chain ids fall back to Ethereum.
   pub fn chain_icon(&self, id: u64, tint: bool) -> Image<'static> {
      let chain = ChainId::new(id).unwrap_or_default();

      let mut img = Image::new(self.chain.for_chain(chain))
         .fit_to_exact_size(Vec2::splat(24.0))
         .show_loading_spinner(false);

      if matches!(chain, ChainId::RobinHood) {
         img = img.corner_radius(10);
      }

      if tint {
         img = img.tint(TINT_1);
      }

      img
   }

   pub fn native_currency_icon(&self, chain: u64, tint: bool) -> Image<'static> {
      let img = match ChainId::new(chain).unwrap_or_default() {
         ChainId::BinanceSmartChain => Image::new(&self.currency.bnb),
         ChainId::Ethereum
         | ChainId::EthereumSepolia
         | ChainId::Optimism
         | ChainId::Base
         | ChainId::Arbitrum
         | ChainId::RobinHood => Image::new(&self.currency.eth),
      };

      tinted(img, tint)
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

   /// Return the token icon (32 x 32) based on its address and chain id
   ///
   /// If it does not exist we return a placeholder.
   /// The texture is loaded lazily on first use to keep startup memory low.
   pub fn token_icon_x32(&self, address: Address, chain_id: u64, tint: bool) -> Image<'static> {
      let key = &(address, chain_id);
      if let Some(icon) = self.tokens.get_or_load_x32(key) {
         tinted(Image::new(&icon), tint)
      } else {
         self.token_placeholder_x32(chain_id, tint)
      }
   }

   /// Return a placeholder icon for a token
   pub fn token_placeholder_x32(&self, id: u64, tint: bool) -> Image<'static> {
      let img = match ChainId::new(id).unwrap_or_default() {
         ChainId::BinanceSmartChain => Image::new(&self.tokens.bep20_x32),
         ChainId::Ethereum
         | ChainId::EthereumSepolia
         | ChainId::Optimism
         | ChainId::Base
         | ChainId::Arbitrum
         | ChainId::RobinHood => Image::new(&self.tokens.erc20_x32),
      };

      tinted(img, tint)
   }

   /// Return the NFT image for a list row (thumbnail), or the placeholder when there is none.
   pub fn nft_icon_x64(
      &self,
      chain_id: u64,
      collection: Address,
      token_id: U256,
      tint: bool,
   ) -> Image<'static> {
      self.nft_icon(collection, chain_id, token_id, false, tint)
   }

   /// Return the NFT image for inspecting a single NFT, or the placeholder when there is none.
   pub fn nft_icon_x250(
      &self,
      chain_id: u64,
      collection: Address,
      token_id: U256,
      tint: bool,
   ) -> Image<'static> {
      self.nft_icon(collection, chain_id, token_id, true, tint)
   }

   /// Return the thumbnail for a collection rather than for one of its tokens, or the placeholder when
   /// none of its art is cached.
   ///
   /// An approval is collection-wide: its log carries no token id, and asking for a fixed one lands on
   /// whatever art *that* token has — usually none — which is how an approval row ended up showing the
   /// placeholder. Any of the collection's cached pictures says what the row is about.
   pub fn nft_collection_icon_x64(
      &self,
      chain_id: u64,
      collection: Address,
      tint: bool,
   ) -> Image<'static> {
      let token_id = self.nfts.cached_id_for_collection(chain_id, collection).unwrap_or(U256::ZERO);

      self.nft_icon(collection, chain_id, token_id, false, tint)
   }

   fn nft_icon(
      &self,
      collection: Address,
      chain_id: u64,
      token_id: U256,
      large: bool,
      tint: bool,
   ) -> Image<'static> {
      let key = (collection, chain_id, token_id);
      let size = if large { 250 } else { 64 };

      let image = if let Some(svg) = self.nfts.svg_source(&key) {
         // Vector art first: egui rasterises SVG at the size the view asks for, so there is no stored
         // texture to look up. The URI must keep its `.svg` extension — that is what tells egui's
         // bytes loader which decoder to use.
         let uri = format!("bytes://nft/{size}/{collection}/{chain_id}/{token_id}.svg");

         Image::new(ImageSource::Bytes {
            uri: uri.into(),
            bytes: svg.into(),
         })
      } else {
         match self.nfts.raster_texture(&key, large) {
            Some(icon) => Image::new(&icon),
            // Untinted: the tint is applied once below, for every branch.
            None => self.nft_placeholder(false),
         }
      };

      // Bounded to the size this variant exists to render at, and that bound is load-bearing: an
      // `Image` left unbounded takes `ImageFit::Fraction([1, 1])` of whatever space it is offered, so
      // in a row it grows to fill the cell and starves the label beside it. The on-chain art is the
      // worst case — an SVG with only a `viewBox` has no intrinsic size at all, so the offer decides
      // how large it is rasterised, and it ends up taking the whole cell.
      tinted(image.max_size(Vec2::splat(size as f32)), tint)
   }

   /// Placeholder shown for an NFT whose image is unknown, or still downloading.
   ///
   /// The same texture serves both sizes: it is a flat graphic, so scaling it costs nothing worth
   /// a second asset.
   pub fn nft_placeholder(&self, tint: bool) -> Image<'static> {
      tinted(Image::new(&self.nfts.placeholder), tint)
   }

   pub fn wallet_main_x24(&self) -> Image<'static> {
      Image::new(&self.misc.wallet_main_x24).sense(Sense::click())
   }
}

/// Apply the muted-tint treatment shared by icons that are shown as fallbacks.
fn tinted(image: Image<'static>, tint: bool) -> Image<'static> {
   match tint {
      true => image.tint(TINT_1),
      false => image,
   }
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

#[cfg(test)]
mod tests {
   use super::*;

   const PLACEHOLDER: &[u8] = include_bytes!("nft/placeholder.png");

   /// A collection-wide row — an approval — takes its art from whichever of the collection's tokens has
   /// some cached. Asking for a fixed id, id 0, showed the placeholder whenever that particular token
   /// had never been fetched, which is exactly what an NFT approval looked like.
   #[test]
   fn a_collection_icon_comes_from_any_cached_token() {
      let ctx = Context::default();
      let icons = Icons::new(&ctx).expect("the bundled icons load");

      let collection = Address::from([0xbc; 20]);

      // Nothing cached: the placeholder, which is a texture and so carries no URI of its own.
      assert!(icons.nft_collection_icon_x64(1, collection, false).uri().is_none());

      // Art cached under a token id that is not 0.
      let key = (collection, 1, U256::from(7));
      icons.nfts.insert_icon(key, NftIconData::Svg(b"<svg/>".to_vec()));

      let uri = icons
         .nft_collection_icon_x64(1, collection, false)
         .uri()
         .map(str::to_string)
         .expect("the cached token's art is an SVG source");
      assert!(
         uri.contains("/7.svg"),
         "expected the collection's cached art, got {uri}"
      );

      // Another chain does not see it: the art is keyed per chain.
      assert!(icons.nft_collection_icon_x64(10, collection, false).uri().is_none());
   }

   /// The placeholder is a binary asset. A corrupt or mis-encoded PNG would otherwise only show up
   /// as a panic at GUI startup, so decode it here.
   #[test]
   fn nft_placeholder_asset_decodes() {
      let image = image::load_from_memory(PLACEHOLDER).expect("placeholder must be a valid image");
      assert_eq!((image.width(), image.height()), (250, 250));
   }

   /// An artwork is bounded by the size its variant promises, not by the space it is offered.
   ///
   /// Nothing else holds it in check: `egui_elements::Label` sizes its image from the room available
   /// to it, and egui's own default fit is `ImageFit::Fraction([1, 1])`, so an unbounded thumbnail
   /// grows to fill a wide cell and squeezes the text of the row it sits in to one character per line.
   #[test]
   fn an_nft_icon_is_bounded_by_its_variant() {
      let ctx = Context::default();
      let icons = Icons::new(&ctx).expect("the bundled icons load");

      // Nothing on disk for this collection, so this is the placeholder branch: what a row shows
      // while the artwork is still being fetched.
      let icon = icons.nft_icon_x64(1, Address::from([0xbb; 20]), U256::from(1), false);

      let mut size = Vec2::ZERO;
      let mut output = ctx.run_ui(eframe::egui::RawInput::default(), |ui| {
         // Far more room than a row leaves a thumbnail, which shares its cell with a label.
         ui.set_max_width(1000.0);
         ui.set_max_height(400.0);
         size = ui.add(icon.clone()).rect.size();
      });
      output.textures_delta.clear();

      assert!(
         size.x <= 64.0 && size.y <= 64.0,
         "an x64 artwork was rendered at {size:?}"
      );
   }

   /// Shorthand for a raster entry. There is no `Default`, because an empty entry is not something
   /// to build by accident.
   fn raster(x64: Vec<u8>, x250: Vec<u8>) -> NftIconData {
      NftIconData::Raster { x64, x250 }
   }

   #[test]
   fn nft_icon_data_knows_when_it_is_empty() {
      assert!(raster(Vec::new(), Vec::new()).is_empty());
      assert!(
         !raster(vec![1], Vec::new()).is_empty(),
         "one rendering is enough to show"
      );

      assert!(!NftIconData::Svg(PLACEHOLDER.to_vec()).is_empty());
      assert!(NftIconData::Svg(Vec::new()).is_empty());
   }

   /// The dedupe contract the fetch path relies on: one download per token, and a miss is not
   /// retried for the rest of the session.
   #[test]
   fn fetch_dedupe_lifecycle() {
      let icons = NftIcons::default();
      let key = (Address::from([0xbc; 20]), 1, U256::from(1));

      assert!(
         icons.try_begin_fetch(&key),
         "the first ask starts a fetch"
      );
      assert!(
         !icons.try_begin_fetch(&key),
         "a second ask while one is in flight must be refused"
      );

      icons.finish_fetch(&key, true);
      assert!(
         !icons.try_begin_fetch(&key),
         "a miss must not be retried this session"
      );

      icons.insert_icon(
         key,
         raster(PLACEHOLDER.to_vec(), PLACEHOLDER.to_vec()),
      );
      assert!(icons.has_icon(&key));
      assert!(
         !icons.try_begin_fetch(&key),
         "an image we already have needs no fetch"
      );

      // The token id is part of the key: a sibling token in the same collection is independent.
      let sibling = (key.0, 1, U256::from(2));
      assert!(icons.try_begin_fetch(&sibling));

      icons.remove_icon(&key);
      assert!(!icons.has_icon(&key));
   }

   /// A missing rendering falls back to the other one instead of dropping to the placeholder.
   #[test]
   fn a_missing_rendering_falls_back_to_the_other() {
      let icons = NftIcons::default();

      let large_only = (Address::from([0xbc; 20]), 1, U256::from(9));
      icons.insert_icon(
         large_only,
         raster(Vec::new(), PLACEHOLDER.to_vec()),
      );
      assert!(
         icons.raster_texture(&large_only, false).is_some(),
         "a thumbnail request must still get the 250px copy"
      );
      assert!(icons.raster_texture(&large_only, true).is_some());

      // Empty data is not an icon: the caller must be told to show the placeholder.
      let empty = (Address::from([0xbc; 20]), 1, U256::from(10));
      icons.insert_icon(empty, raster(Vec::new(), Vec::new()));
      assert!(icons.raster_texture(&empty, false).is_none());
      assert!(icons.raster_texture(&empty, true).is_none());
      assert!(!icons.has_icon(&empty));
   }

   /// `needs_fetch` is the read-only twin of `try_begin_fetch`, used to *count* the downloads a list
   /// load starts. It must report false for art we have, for a download in flight, and for a token
   /// that already missed this session — otherwise a capped loader spends its whole budget re-asking
   /// about the same rows and never reaches the tail of a long list.
   #[test]
   fn needs_fetch_mirrors_try_begin_fetch_without_claiming() {
      let icons = NftIcons::default();
      let key = (Address::from([0xbc; 20]), 1, U256::from(21));

      assert!(
         icons.needs_fetch(&key),
         "nothing known: a fetch is wanted"
      );
      assert!(
         icons.needs_fetch(&key),
         "probing must not claim it"
      );

      assert!(icons.try_begin_fetch(&key));
      assert!(
         !icons.needs_fetch(&key),
         "one is already in flight"
      );

      icons.finish_fetch(&key, true);
      assert!(!icons.needs_fetch(&key), "it missed this session");

      let with_art = (Address::from([0xbc; 20]), 1, U256::from(22));
      icons.insert_icon(
         with_art,
         raster(PLACEHOLDER.to_vec(), PLACEHOLDER.to_vec()),
      );
      assert!(
         !icons.needs_fetch(&with_art),
         "we already have it"
      );
   }

   /// Vector art has no raster renderings to build a texture from, so the texture path must report
   /// "nothing" instead of panicking — the view checks the SVG path first, and this is what stops a
   /// future refactor from silently reordering those two checks.
   #[test]
   fn vector_art_is_served_as_source_not_as_a_texture() {
      let icons = NftIcons::default();
      let key = (Address::from([0xbc; 20]), 1, U256::from(11));
      let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"8\" height=\"8\"/>";

      icons.insert_icon(key, NftIconData::Svg(svg.to_vec()));

      assert!(icons.raster_texture(&key, false).is_none());
      assert!(icons.raster_texture(&key, true).is_none());
      assert_eq!(icons.svg_source(&key).as_deref(), Some(&svg[..]));
      assert!(
         icons.has_icon(&key),
         "vector art counts as an image we have"
      );
   }
}
