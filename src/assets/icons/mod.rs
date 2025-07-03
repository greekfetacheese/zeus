#![allow(dead_code)]
#![allow(unused_variables)]

use eframe::egui::{
   ColorImage, Context, Image, Sense, TextureHandle, epaint::textures::TextureOptions,
};

use image::imageops::FilterType;
use std::collections::HashMap;
use std::str::FromStr;
use zeus_eth::{alloy_primitives::Address, currency::Currency};
use zeus_token_list::*;

use bincode::{config::standard, decode_from_slice};

/// Icons used in the GUI
pub struct Icons {
   pub chain: ChainIcons,
   pub currency: CurrencyIcons,
   pub tokens: TokenIcons,
   pub misc: MiscIcons,
}

pub struct TokenIcons {
   pub icon_x32: HashMap<(Address, u64), TextureHandle>,
   // pub icon_x24: HashMap<(Address, u64), TextureHandle>,
   /// ERC20 Placeholder
   pub erc20_x32: TextureHandle,
   /// BEP20 Placeholder
   pub bep20_x32: TextureHandle,
}

impl TokenIcons {
   pub fn new(ctx: &Context) -> Result<Self, anyhow::Error> {

      let (icon_data, _bytes_read): (Vec<TokenData>, usize) =
         decode_from_slice(TOKEN_DATA, standard())?;

      let mut icons = HashMap::new();

      let texture_options = TextureOptions::default();
      for icon in icon_data {
         let img = load_image(&icon.icon_data)?;
         let texture_handle = ctx.load_texture(icon.address.to_string(), img, texture_options);
         icons.insert(
            (Address::from_str(&icon.address)?, icon.chain_id),
            texture_handle,
         );
      }

      // ERC20 & BEP20 Placeholders
      let erc20 = load_image(include_bytes!("currency/resized/erc20.png"))?;
      let bep20 = load_image(include_bytes!("currency/resized/bep20.png"))?;
      let erc20 = ctx.load_texture("erc20", erc20, texture_options);
      let bep20 = ctx.load_texture("bep20", bep20, texture_options);

      Ok(Self {
         icon_x32: icons,
         erc20_x32: erc20,
         bep20_x32: bep20,
      })
   }
}

pub struct ChainIcons {
   pub eth_x24: TextureHandle,
   pub op_x24: TextureHandle,
   pub bsc_x24: TextureHandle,
   pub base_x24: TextureHandle,
   pub arbitrum_x24: TextureHandle,
   pub eth_x16: TextureHandle,
   pub op_x16: TextureHandle,
   pub bsc_x16: TextureHandle,
   pub base_x16: TextureHandle,
   pub arbitrum_x16: TextureHandle,
}
impl ChainIcons {
   pub fn new(ctx: &Context) -> Result<Self, anyhow::Error> {
      let texture_options = TextureOptions::default();

      let eth_x24 = load_and_resize_image(include_bytes!("chain/ethereum.png"), 24, 24)?;
      let bsc_x24 = load_and_resize_image(include_bytes!("chain/bsc.png"), 24, 24)?;
      let op_x24 = load_and_resize_image(include_bytes!("chain/op.png"), 24, 24)?;
      let base_x24 = load_and_resize_image(include_bytes!("chain/base.png"), 24, 24)?;
      let arbitrum_x24 = load_and_resize_image(include_bytes!("chain/arbitrum.png"), 24, 24)?;

      let eth_x16 = load_and_resize_image(include_bytes!("chain/ethereum.png"), 16, 16)?;
      let bsc_x16 = load_and_resize_image(include_bytes!("chain/bsc.png"), 16, 16)?;
      let op_x16 = load_and_resize_image(include_bytes!("chain/op.png"), 16, 16)?;
      let base_x16 = load_and_resize_image(include_bytes!("chain/base.png"), 16, 16)?;
      let arbitrum_x16 = load_and_resize_image(include_bytes!("chain/arbitrum.png"), 16, 16)?;

      Ok(Self {
         eth_x24: ctx.load_texture("eth", eth_x24, texture_options),
         op_x24: ctx.load_texture("op", op_x24, texture_options),
         bsc_x24: ctx.load_texture("bsc", bsc_x24, texture_options),
         base_x24: ctx.load_texture("base", base_x24, texture_options),
         arbitrum_x24: ctx.load_texture("arbitrum", arbitrum_x24, texture_options),
         eth_x16: ctx.load_texture("eth_x16", eth_x16, texture_options),
         op_x16: ctx.load_texture("op_x16", op_x16, texture_options),
         bsc_x16: ctx.load_texture("bsc_x16", bsc_x16, texture_options),
         base_x16: ctx.load_texture("base_x16", base_x16, texture_options),
         arbitrum_x16: ctx.load_texture("arbitrum_x16", arbitrum_x16, texture_options),
      })
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
   pub red_circle: TextureHandle,
   pub green_circle: TextureHandle,
   pub swap: TextureHandle,
   pub view: TextureHandle,
   pub view_light: TextureHandle,
   pub hide: TextureHandle,
   pub hide_light: TextureHandle,
}

impl MiscIcons {
   pub fn new(ctx: &Context) -> Result<Self, anyhow::Error> {
      let texture_options = TextureOptions::default();

      let red_circle = load_and_resize_image(include_bytes!("misc/red-circle.png"), 16, 16)?;
      let green_circle = load_and_resize_image(include_bytes!("misc/green-circle.png"), 16, 16)?;
      let swap = load_and_resize_image(include_bytes!("misc/swap.png"), 24, 24)?;
      let view = load_and_resize_image(include_bytes!("misc/view.png"), 24, 24)?;
      let view_light = load_and_resize_image(include_bytes!("misc/view-light.png"), 24, 24)?;
      let hide = load_and_resize_image(include_bytes!("misc/hide.png"), 24, 24)?;
      let hide_light = load_and_resize_image(include_bytes!("misc/hide-light.png"), 24, 24)?;

      Ok(Self {
         red_circle: ctx.load_texture("red_circle", red_circle, texture_options),
         green_circle: ctx.load_texture("green_circle", green_circle, texture_options),
         swap: ctx.load_texture("swap", swap, texture_options),
         view: ctx.load_texture("view", view, texture_options),
         hide: ctx.load_texture("hide", hide, texture_options),
         view_light: ctx.load_texture("view_light", view_light, texture_options),
         hide_light: ctx.load_texture("hide_light", hide_light, texture_options),
      })
   }
}

impl Icons {
   pub fn new(ctx: &Context) -> Result<Self, anyhow::Error> {
      let texture_options = TextureOptions::default();

      let chain_icons = ChainIcons::new(ctx)?;
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
   pub fn chain_icon(&self, id: u64) -> Image<'static> {
      match id {
         1 => Image::new(&self.chain.eth_x24),
         10 => Image::new(&self.chain.op_x24),
         56 => Image::new(&self.chain.bsc_x24),
         8453 => Image::new(&self.chain.base_x24),
         42161 => Image::new(&self.chain.arbitrum_x24),
         _ => Image::new(&self.chain.eth_x24),
      }
   }

   pub fn chain_icon_x16(&self, id: u64) -> Image<'static> {
      match id {
         1 => Image::new(&self.chain.eth_x16),
         10 => Image::new(&self.chain.op_x16),
         56 => Image::new(&self.chain.bsc_x16),
         8453 => Image::new(&self.chain.base_x16),
         42161 => Image::new(&self.chain.arbitrum_x16),
         _ => Image::new(&self.chain.eth_x16),
      }
   }

   pub fn eth_black(&self) -> Image<'static> {
      Image::new(&self.currency.eth_black)
   }

   pub fn eth_black_x24(&self) -> Image<'static> {
      Image::new(&self.currency.eth_black_x24)
   }

   pub fn eth_x24(&self) -> Image<'static> {
      Image::new(&self.currency.eth_x24)
   }

   pub fn bnb_x24(&self) -> Image<'static> {
      Image::new(&self.currency.bnb_x24)
   }

   pub fn native_currency_icon(&self, chain: u64) -> Image<'static> {
      match chain {
         56 => Image::new(&self.currency.bnb),
         _ => Image::new(&self.currency.eth),
      }
   }

   pub fn native_currency_icon_x24(&self, chain: u64) -> Image<'static> {
      match chain {
         56 => Image::new(&self.currency.bnb_x24),
         _ => Image::new(&self.currency.eth_x24),
      }
   }

   /// Return the currency icon based on the currency
   ///
   /// If the currency is native, it will return the native currency icon based on the chain_id
   ///
   /// If its ERC20, it will return the token icon based on the token address and chain id
   pub fn currency_icon(&self, currency: &Currency) -> Image<'static> {
      if currency.is_native() {
         self.native_currency_icon(currency.native().unwrap().chain_id)
      } else {
         let token = currency.erc20().unwrap();
         self.token_icon(token.address, token.chain_id)
      }
   }

   /// Return the token icon (32 x 32) based on its address and chain id
   ///
   /// If it does not exist we return a placeholder
   pub fn token_icon(&self, address: Address, chain_id: u64) -> Image<'static> {
      let key = &(address, chain_id);
      if let Some(icon) = self.tokens.icon_x32.get(key) {
         return Image::new(icon);
      } else {
         self.token_placeholder(chain_id)
      }
   }

   /// Return a placeholder icon for a token
   pub fn token_placeholder(&self, id: u64) -> Image<'static> {
      match id {
         56 => Image::new(&self.tokens.bep20_x32),
         _ => Image::new(&self.tokens.erc20_x32),
      }
   }

   /// Return the erc20 icon
   pub fn erc20_icon(&self) -> Image<'static> {
      Image::new(&self.tokens.erc20_x32)
   }

   /// Return the bep20 icon
   pub fn bep20_icon(&self) -> Image<'static> {
      Image::new(&self.tokens.bep20_x32)
   }

   pub fn red_circle(&self) -> Image<'static> {
      Image::new(&self.misc.red_circle)
   }

   pub fn green_circle(&self) -> Image<'static> {
      Image::new(&self.misc.green_circle)
   }

   pub fn swap(&self) -> Image<'static> {
      Image::new(&self.misc.swap).sense(Sense::click())
   }

   pub fn view(&self) -> Image<'static> {
      Image::new(&self.misc.view).sense(Sense::click())
   }

   pub fn hide(&self) -> Image<'static> {
      Image::new(&self.misc.hide).sense(Sense::click())
   }

   pub fn view_light(&self) -> Image<'static> {
      Image::new(&self.misc.view_light).sense(Sense::click())
   }

   pub fn hide_light(&self) -> Image<'static> {
      Image::new(&self.misc.hide_light).sense(Sense::click())
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
