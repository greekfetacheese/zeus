#![allow(dead_code)]
#![allow(unused_variables)]

use eframe::egui::{
    epaint::textures::TextureOptions,
    ColorImage,
    Context,
    Image,
    Sense,
    TextureHandle,
};

use zeus_eth::alloy_primitives::Address;
use zeus_token_list::*;
use image::imageops::FilterType;
use std::collections::HashMap;
use std::str::FromStr;

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
    pub icon: HashMap<(Address, u64), TextureHandle>,
}

impl TokenIcons {
    pub fn new(ctx: &Context) -> Result<Self, anyhow::Error> {
        let icon_data: Vec<TokenIconData> = serde_json::from_str(TOKEN_ICONS)?;

        let mut icons = HashMap::new();

        let texture_options = TextureOptions::default();
        for icon in icon_data {
            let img = load_and_resize_image(&icon.icon_data, 32, 32)?;
            let texture_handle = ctx.load_texture(icon.address.to_string(), img, texture_options);
            icons.insert((Address::from_str(&icon.address)?, icon.chain_id), texture_handle);
        }
        Ok(Self {
            icon: icons,
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
}

#[derive(Clone)]
pub struct CurrencyIcons {
    pub eth: TextureHandle,
    pub bnb: TextureHandle,
}

#[derive(Clone)]
pub struct MiscIcons {
    pub add_wallet: TextureHandle,
    pub wallet: TextureHandle,
    pub copy: TextureHandle,
    pub settings: TextureHandle,
    pub right_arrow: TextureHandle,
    pub right_arrow2: TextureHandle,
    pub arrow_back: TextureHandle,
    pub contact: TextureHandle,
    pub trash: TextureHandle,
}

impl Icons {
    pub fn new(ctx: &Context) -> Result<Self, anyhow::Error> {
        // Chain icons
        let eth_icon = load_image(include_bytes!("chain/resized/ethereum.png"))?;
        let op_icon = load_image(include_bytes!("chain/resized/op.png"))?;
        let bsc_icon = load_image(include_bytes!("chain/resized/bsc.png"))?;
        let base_icon = load_image(include_bytes!("chain/resized/base.png"))?;
        let arbitrum_icon = load_image(include_bytes!("chain/resized/arbitrum.png"))?;

        // Currency icons
        let eth_coin = load_image(include_bytes!("currency/resized/ethereum.png"))?;
        let bnb_coin = load_image(include_bytes!("currency/resized/bnb.png"))?;

        let erc20 = load_image(include_bytes!("currency/resized/erc20.png"))?;
        let bep20 = load_image(include_bytes!("currency/resized/bep20.png"))?;

        // Misc Icons
        let wallet_add = load_image(
            include_bytes!("wallet/resized/wallet-plus.png"))?;
        let wallet = load_image(include_bytes!("wallet/resized/wallet.png"))?;
        let copy = load_image(include_bytes!("misc/resized/copy.png"))?;
        let settings = load_image(include_bytes!("misc/resized/settings.png"))?;
        let right_arrow = load_image(include_bytes!("misc/resized/arrow-right.png"))?;
        let right_arrow2 = load_image(include_bytes!("misc/resized/arrow-right2.png"))?;
        let arrow_back = load_image(include_bytes!("misc/resized/arrow-back.png"))?;
        let contact = load_image(include_bytes!("misc/resized/contact.png"))?;
        let trash = load_image(include_bytes!("misc/resized/trash.png"))?;

        let texture_options = TextureOptions::default();

        let chain_icons = ChainIcons {
            eth: ctx.load_texture("eth", eth_icon, texture_options),
            op: ctx.load_texture("op", op_icon, texture_options),
            bsc: ctx.load_texture("bsc", bsc_icon, texture_options),
            base: ctx.load_texture("base", base_icon, texture_options),
            arbitrum: ctx.load_texture("arbitrum", arbitrum_icon, texture_options),
        };

        let currency_icons = CurrencyIcons {
            eth: ctx.load_texture("eth_coin", eth_coin, texture_options),
            bnb: ctx.load_texture("bnb_coin", bnb_coin, texture_options),
        };

        let erc20 = ctx.load_texture("erc20", erc20, texture_options);
        let bep20 = ctx.load_texture("bep20", bep20, texture_options);

        let misc_icons = MiscIcons {
            add_wallet: ctx.load_texture("add_wallet", wallet_add, texture_options),
            wallet: ctx.load_texture("wallet", wallet, texture_options),
            copy: ctx.load_texture("copy", copy, texture_options),
            settings: ctx.load_texture("settings", settings, texture_options),
            right_arrow: ctx.load_texture("right_arrow", right_arrow, texture_options),
            right_arrow2: ctx.load_texture("right_arrow2", right_arrow2, texture_options),
            contact: ctx.load_texture("contact", contact, texture_options),
            arrow_back: ctx.load_texture("arrow_back", arrow_back, texture_options),
            trash: ctx.load_texture("trash", trash, texture_options),
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

    /// Return the native currency icon based on the chain_id
    pub fn currency_icon(&self, id: u64) -> Image<'static> {
        match id {
            56 => Image::new(&self.currency.bnb),
            _ => Image::new(&self.currency.eth),
        }
    }

    /// Return the token icon based on its address and chain id
    ///
    /// If it does not exist we return a placeholder
    pub fn token_icon(&self, address: Address, chain_id: u64) -> Image<'static> {
        let key = &(address, chain_id);
        if let Some(icon) = self.tokens.icon.get(key) {
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

    /// Return the copy icon
    pub fn copy(&self) -> Image<'static> {
        Image::new(&self.misc.copy).sense(Sense::click())
    }

    /// Return the settings icons
    pub fn settings(&self) -> Image<'static> {
        Image::new(&self.misc.settings).sense(Sense::click())
    }

    pub fn right_arrow(&self) -> Image<'static> {
        Image::new(&self.misc.right_arrow).sense(Sense::click())
    }

    pub fn right_arrow2(&self) -> Image<'static> {
        Image::new(&self.misc.right_arrow2).sense(Sense::click())
    }

    pub fn arrow_back(&self) -> Image<'static> {
        Image::new(&self.misc.arrow_back).sense(Sense::click())
    }

    pub fn contact(&self) -> Image<'static> {
        Image::new(&self.misc.contact).sense(Sense::click())
    }

    pub fn trash(&self) -> Image<'static> {
        Image::new(&self.misc.trash).sense(Sense::click())
    }
}

fn load_and_resize_image(
    image_data: &[u8],
    width: u32,
    height: u32
) -> Result<ColorImage, image::ImageError> {
    let image = image::load_from_memory(image_data)?;
    let resized_image = image.resize(width, height, FilterType::Lanczos3);
    let size = [resized_image.width() as _, resized_image.height() as _];
    let image_buffer = resized_image.to_rgba8();
    let pixels = image_buffer.as_flat_samples();
    Ok(ColorImage::from_rgba_unmultiplied(size, pixels.as_slice()))
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