use std::sync::{ Arc, RwLock };
use providers::{ RpcProviders, Rpc };
use crate::core::{ Profile, Wallet, user::Portfolio, utils::{ HttpClient, get_http_client } };
use zeus_eth::alloy_primitives::{ Address, U256 };
use zeus_eth::prelude::{Currency, ChainId, UniswapV2Pool, UniswapV3Pool};

pub mod providers;
pub mod db;

#[derive(Clone)]
pub struct ZeusCtx(Arc<RwLock<ZeusContext>>);

impl ZeusCtx {
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(ZeusContext::new())))
    }

    /// Shared access to the context
    pub fn read<R>(&self, reader: impl FnOnce(&ZeusContext) -> R) -> R {
        reader(&self.0.read().unwrap())
    }

    /// Exclusive mutable access to the context
    pub fn write<R>(&self, writer: impl FnOnce(&mut ZeusContext) -> R) -> R {
        writer(&mut self.0.write().unwrap())
    }

    pub fn profile_exists(&self) -> bool {
        self.read(|ctx| ctx.profile_exists)
    }

    pub fn logged_in(&self) -> bool {
        self.read(|ctx| ctx.logged_in)
    }

    pub fn profile(&self) -> Profile {
        self.read(|ctx| ctx.profile.clone())
    }

    pub fn rpc(&self) -> Rpc {
        self.read(|ctx| ctx.rpc.clone())
    }

    pub fn get_client(&self) -> Result<HttpClient, anyhow::Error> {
        self.read(|ctx| ctx.get_client())
    }

    pub fn get_client_with_id(&self, id: u64) -> Result<HttpClient, anyhow::Error> {
        self.read(|ctx| ctx.get_client_with_id(id))
    }

    pub fn wallet(&self) -> Wallet {
        self.read(|ctx| ctx.wallet())
    }

    pub fn chain(&self) -> ChainId {
        self.read(|ctx| ctx.chain.clone())
    }

    pub fn save_db(&self) -> Result<(), anyhow::Error> {
        self.read(|ctx| ctx.db.save_to_file())
    }

    pub fn get_token_balance(&self, owner: Address, token: Address) -> U256 {
        self.read(|ctx| ctx.db.get_token_balance(ctx.chain.id(), owner, token))
    }

    pub fn get_eth_balance(&self, owner: Address) -> U256 {
        self.read(|ctx| ctx.db.get_eth_balance(ctx.chain.id(), owner))
    }

    pub fn get_currencies(&self, chain: u64) -> Arc<Vec<Currency>> {
        self.read(|ctx| ctx.db.get_currencies(chain))
    }

    pub fn get_portfolio(&self, owner: Address) -> Arc<Portfolio> {
        self.read(|ctx| ctx.db.get_portfolio(ctx.chain.id(), owner))
    }

    pub fn get_token_price(&self, token: Address) -> f64 {
        self.read(|ctx| ctx.db.get_price(ctx.chain.id(), token))
    }

    pub fn get_v2_pool(&self, token_a: Address, token_b: Address) -> Option<UniswapV2Pool> {
        self.read(|ctx| ctx.db.get_v2_pool(ctx.chain.id(), token_a, token_b))
    }

    pub fn get_v3_pool(&self, fee: u32, token_a: Address, token_b: Address) -> Option<UniswapV3Pool> {
        self.read(|ctx| ctx.db.get_v3_pool(ctx.chain.id(), fee, token_a, token_b))
    }
}

pub struct ZeusContext {

    pub providers: RpcProviders,

    /// The current selected rpc provider from the GUI
    pub rpc: Rpc,

    /// The current selected chain from the GUI
    pub chain: ChainId,

    /// Loaded profile
    pub profile: Profile,

    pub profile_exists: bool,

    pub logged_in: bool,

    pub db: db::ZeusDB,
}

impl ZeusContext {
    pub fn new() -> Self {
        let mut providers = RpcProviders::default();
        if let Ok(loaded_providers) = RpcProviders::load_from_file() {
            providers.rpc = loaded_providers.rpc;
        }

        // This should not panic
        let profile_exists = Profile::exists().expect("Failed to read data directory");
        let rpc = providers.get(1).expect("Failed to find provider");

        let db = if let Ok(db) = db::ZeusDB::load_from_file() {
            db
        } else {
            let mut db = db::ZeusDB::default();
            db.load_default_currencies().unwrap();
            db
        };

        Self {
            providers,
            rpc,
            chain: ChainId::new(1).unwrap(),
            profile: Profile::default(),
            profile_exists,
            logged_in: false,
            db,
        }
    }

    pub fn get_client(&self) -> Result<HttpClient, anyhow::Error> {
        let rpc = self.providers.get(self.chain.id())?;
        let client = get_http_client(&rpc.url)?;
        Ok(client)
    }

    pub fn get_client_with_id(&self, id: u64) -> Result<HttpClient, anyhow::Error> {
        let rpc = self.providers.get(id)?;
        let client = get_http_client(&rpc.url)?;
        Ok(client)
    }

    /// Get the current wallet selected from the GUI
    pub fn wallet(&self) -> Wallet {
        self.profile.current_wallet.clone()
    }
}