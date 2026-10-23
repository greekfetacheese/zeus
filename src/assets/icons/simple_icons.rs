use super::load_image;
use super::static_bytes_source;
use eframe::egui::Context;
use eframe::egui::ImageSource;
use eframe::egui::TextureHandle;
use eframe::egui::epaint::textures::TextureOptions;
use zeus_eth::types::ChainId;

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
