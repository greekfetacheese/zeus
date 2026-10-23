//! The GUI's icon stores.
//!
//! This file is the outline: [`Icons`], the accessors that pick an icon for a view, and the helpers
//! the stores share. Each store's art and cache live in its own leaf module, so a leaf can hold its
//! own tests next to the private state they read.

use eframe::egui::{ColorImage, Context, Image, ImageSource, Sense, Vec2};
use egui_elements::utils::TINT_1;
use std::borrow::Cow;
use zeus_eth::types::ChainId;
use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::Currency,
};

mod disk;
mod nft_icons;
mod simple_icons;
mod token_icons;

pub(crate) use disk::{delete_nft_icon, delete_token_icon, save_nft_icon, save_token_icon};
pub use nft_icons::{NftIconData, NftIcons};
pub use simple_icons::{ChainIcons, CurrencyIcons, MiscIcons};
pub use token_icons::TokenIcons;

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

/// Key for one NFT's images: `(collection, chain id, token id)`.
pub type NftKey = (Address, u64, U256);

impl Icons {
   pub fn new(ctx: &Context) -> Result<Self, anyhow::Error> {
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

      let image = match self.nfts.raster_texture(&key, large) {
         Some(icon) => Image::new(&icon),
         // Untinted: the tint is applied once below.
         None => self.nft_placeholder(false),
      };

      // Bounded to the size this variant exists to render at, and that bound is load-bearing: an
      // `Image` left unbounded takes `ImageFit::Fraction([1, 1])` of whatever space it is offered, so
      // in a row it grows to fill the cell and starves the label beside it.
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
}
