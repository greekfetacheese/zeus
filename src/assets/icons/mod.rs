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

/// Icons used in the GUI
#[derive(Clone)]
pub struct Icons {
   pub chain: ChainIcons,
   pub currency: CurrencyIcons,
   pub erc20: TextureHandle,
   pub bep20: TextureHandle,
   pub tokens: TokenIcons,
   pub misc: MiscIcons,
}

#[derive(Clone, Default)]
pub struct TokenIcons {
   pub icon_x32: HashMap<(Address, u64), TextureHandle>,
   pub icon_x24: HashMap<(Address, u64), TextureHandle>,
}

impl TokenIcons {
   pub fn new(ctx: &Context) -> Result<Self, anyhow::Error> {
      let icon_data: Vec<TokenIconData> = serde_json::from_str(TOKEN_ICONS)?;

      let mut icons = HashMap::new();
      let mut icons_x24 = HashMap::new();

      let texture_options = TextureOptions::default();
      for icon in icon_data {
         let img = load_and_resize_image(&icon.icon_data, 32, 32)?;
         let img_x24 = load_and_resize_image(&icon.icon_data, 24, 24)?;
         let texture_handle = ctx.load_texture(icon.address.to_string(), img, texture_options);
         let texture_handle_x24 =
            ctx.load_texture(icon.address.to_string(), img_x24, texture_options);
         icons_x24.insert(
            (Address::from_str(&icon.address)?, icon.chain_id),
            texture_handle_x24,
         );

         icons.insert(
            (Address::from_str(&icon.address)?, icon.chain_id),
            texture_handle,
         );
      }
      Ok(Self {
         icon_x32: icons,
         icon_x24: icons_x24,
      })
   }
}

#[derive(Clone)]
pub struct ChainIcons {
   pub eth: TextureHandle,
   pub op: TextureHandle,
   pub bsc: TextureHandle,
   pub base: TextureHandle,
   pub arbitrum: TextureHandle,
   pub eth_x16: TextureHandle,
   pub op_x16: TextureHandle,
   pub bsc_x16: TextureHandle,
   pub base_x16: TextureHandle,
   pub arbitrum_x16: TextureHandle,
}

#[derive(Clone)]
pub struct CurrencyIcons {
   pub eth: TextureHandle,
   pub eth_black: TextureHandle,
   pub eth_black_x24: TextureHandle,
   pub eth_x24: TextureHandle,
   pub bnb: TextureHandle,
   pub bnb_x24: TextureHandle,
}

#[derive(Clone)]
pub struct MiscIcons {
   pub add_wallet: TextureHandle,
   pub wallet: TextureHandle,
   pub arrow_left: TextureHandle,
   pub trash: TextureHandle,
   pub edit: TextureHandle,
   pub down: TextureHandle,
   pub right: TextureHandle,
   pub red_circle: TextureHandle,
   pub green_circle: TextureHandle,
   pub swap: TextureHandle,
}

impl Icons {
   pub fn new(ctx: &Context) -> Result<Self, anyhow::Error> {
      // Chain icons
      let eth_icon = load_image(include_bytes!("chain/resized/ethereum.png"))?;
      let op_icon = load_image(include_bytes!("chain/resized/op.png"))?;
      let bsc_icon = load_image(include_bytes!("chain/resized/bsc.png"))?;
      let base_icon = load_image(include_bytes!("chain/resized/base.png"))?;
      let arbitrum_icon = load_image(include_bytes!("chain/resized/arbitrum.png"))?;
      let eth_x16 = load_image(include_bytes!("chain/resized/x16/ethereum.png"))?;
      let op_x16 = load_image(include_bytes!("chain/resized/x16/op.png"))?;
      let bsc_x16 = load_image(include_bytes!("chain/resized/x16/bsc.png"))?;
      let base_x16 = load_image(include_bytes!("chain/resized/x16/base.png"))?;
      let arbitrum_x16 = load_image(include_bytes!("chain/resized/x16/arbitrum.png"))?;

      // Currency icons
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

      // ERC20 & BEP20 Placeholders
      let erc20 = load_image(include_bytes!("currency/resized/erc20.png"))?;
      let bep20 = load_image(include_bytes!("currency/resized/bep20.png"))?;

      // Misc Icons
      let wallet_add = load_image(include_bytes!("wallet/resized/wallet-plus.png"))?;
      let wallet = load_image(include_bytes!("wallet/resized/wallet.png"))?;
      let arrow_left = load_image(include_bytes!("misc/resized/arrow-left.png"))?;
      let trash = load_image(include_bytes!("misc/resized/trash.png"))?;
      let edit = load_image(include_bytes!("misc/resized/edit.png"))?;
      let down = load_image(include_bytes!("misc/down.png"))?;
      let right = load_image(include_bytes!("misc/right.png"))?;
      let red_circle = load_and_resize_image(include_bytes!("misc/red-circle.png"), 16, 16)?;
      let green_circle = load_and_resize_image(include_bytes!("misc/green-circle.png"), 16, 16)?;
      let swap = load_and_resize_image(include_bytes!("misc/swap.png"), 24, 24)?;
      let texture_options = TextureOptions::default();

      let chain_icons = ChainIcons {
         eth: ctx.load_texture("eth", eth_icon, texture_options),
         op: ctx.load_texture("op", op_icon, texture_options),
         bsc: ctx.load_texture("bsc", bsc_icon, texture_options),
         base: ctx.load_texture("base", base_icon, texture_options),
         arbitrum: ctx.load_texture("arbitrum", arbitrum_icon, texture_options),
         eth_x16: ctx.load_texture("eth_x16", eth_x16, texture_options),
         op_x16: ctx.load_texture("op_x16", op_x16, texture_options),
         bsc_x16: ctx.load_texture("bsc_x16", bsc_x16, texture_options),
         base_x16: ctx.load_texture("base_x16", base_x16, texture_options),
         arbitrum_x16: ctx.load_texture("arbitrum_x16", arbitrum_x16, texture_options),
      };

      let currency_icons = CurrencyIcons {
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
      };

      let erc20 = ctx.load_texture("erc20", erc20, texture_options);
      let bep20 = ctx.load_texture("bep20", bep20, texture_options);

      let misc_icons = MiscIcons {
         add_wallet: ctx.load_texture("add_wallet", wallet_add, texture_options),
         wallet: ctx.load_texture("wallet", wallet, texture_options),
         arrow_left: ctx.load_texture("arrow_left", arrow_left, texture_options),
         trash: ctx.load_texture("trash", trash, texture_options),
         edit: ctx.load_texture("edit", edit, texture_options),
         down: ctx.load_texture("down", down, texture_options),
         right: ctx.load_texture("right", right, texture_options),
         red_circle: ctx.load_texture("red_circle", red_circle, texture_options),
         green_circle: ctx.load_texture("green_circle", green_circle, texture_options),
         swap: ctx.load_texture("swap", swap, texture_options),
      };

      Ok(Self {
         chain: chain_icons,
         currency: currency_icons,
         erc20,
         bep20,
         tokens: TokenIcons::new(ctx)?,
         misc: misc_icons,
      })
   }

   /// Return the chain icon based on the chain_id
   pub fn chain_icon(&self, id: &u64) -> Image<'static> {
      match id {
         1 => Image::new(&self.chain.eth),
         10 => Image::new(&self.chain.op),
         56 => Image::new(&self.chain.bsc),
         8453 => Image::new(&self.chain.base),
         42161 => Image::new(&self.chain.arbitrum),
         _ => Image::new(&self.chain.eth),
      }
   }

   pub fn chain_icon_x16(&self, id: &u64) -> Image<'static> {
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

   pub fn currency_icon_x24(&self, currency: &Currency) -> Image<'static> {
      if currency.is_native() {
         self.native_currency_icon_x24(currency.native().unwrap().chain_id)
      } else {
         let token = currency.erc20().unwrap();
         self.token_icon_x24(token.address, token.chain_id)
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

   /// Return the token icon (24 x 24) based on its address and chain id
   ///
   /// If it does not exist we return a placeholder
   pub fn token_icon_x24(&self, address: Address, chain_id: u64) -> Image<'static> {
      let key = &(address, chain_id);
      if let Some(icon) = self.tokens.icon_x24.get(key) {
         return Image::new(icon);
      } else {
         self.token_placeholder(chain_id)
      }
   }

   /// Return a placeholder icon for a token
   pub fn token_placeholder(&self, id: u64) -> Image<'static> {
      match id {
         56 => Image::new(&self.bep20),
         _ => Image::new(&self.erc20),
      }
   }

   /// Return the erc20 icon
   pub fn erc20_icon(&self) -> Image<'static> {
      Image::new(&self.erc20)
   }

   /// Return the bep20 icon
   pub fn bep20_icon(&self) -> Image<'static> {
      Image::new(&self.bep20)
   }

   /// Return the add wallet icon
   pub fn add_wallet_icon(&self) -> Image<'static> {
      Image::new(&self.misc.add_wallet).sense(Sense::click())
   }

   /// Return the wallet icon
   pub fn wallet(&self) -> Image<'static> {
      Image::new(&self.misc.wallet).sense(Sense::click())
   }

   pub fn arrow_left(&self) -> Image<'static> {
      Image::new(&self.misc.arrow_left).sense(Sense::click())
   }

   pub fn arrow_down(&self) -> Image<'static> {
      Image::new(&self.misc.down).sense(Sense::click())
   }

   pub fn arrow_right(&self) -> Image<'static> {
      Image::new(&self.misc.right).sense(Sense::click())
   }

   pub fn trash(&self) -> Image<'static> {
      Image::new(&self.misc.trash).sense(Sense::click())
   }

   pub fn edit(&self) -> Image<'static> {
      Image::new(&self.misc.edit).sense(Sense::click())
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
