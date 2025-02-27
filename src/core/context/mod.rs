use std::sync::{ Arc, RwLock };
use db::Contact;
use providers::{ RpcProviders, Rpc };
use crate::core::{ Profile, Wallet, user::Portfolio };
use zeus_eth::alloy_primitives::{ Address, U256 };
use zeus_eth::{types::ChainId, currency::{Currency, erc20::ERC20Token}};
use zeus_eth::amm::{pool_manager::PoolStateManagerHandle, uniswap::{v2::pool::UniswapV2Pool, v3::pool::{FEE_TIERS, UniswapV3Pool}}};
use super::utils::pool_data_dir;
use zeus_eth::utils::client::{get_http_client, HttpClient};

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

    pub fn pool_manager(&self) -> PoolStateManagerHandle {
        self.read(|ctx| ctx.pool_manager.clone())
    }

    pub fn save_pool_data(&self) -> Result<(), anyhow::Error> {
        let data = self.read(|ctx| ctx.pool_manager.to_string().ok());
        if let Some(data) = data {
            let dir = pool_data_dir()?;
            std::fs::write(dir, data)?;
        }
        Ok(())
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

    pub fn get_portfolio(&self, chain: u64, owner: Address) -> Option<Arc<Portfolio>> {
        self.read(|ctx| ctx.db.get_portfolio(chain, owner))
    }

    pub fn get_portfolio_value(&self, chain: u64, owner: Address) -> f64 {
        self.read(|ctx| ctx.db.get_portfolio(chain, owner).map(|p| p.value).unwrap_or(0.0))
    }

    pub fn update_portfolio_value(&self, chain: u64, owner: Address, value: f64) {
        self.write(|ctx| {
            if let Some(portfolio) = ctx.db.get_portfolio_mut(chain, owner) {
                portfolio.value = value;
            }
        })
    }

    pub fn contacts(&self) -> Vec<Contact> {
        self.read(|ctx| ctx.db.contacts.clone())
    }

    pub fn get_token_price(&self, token: &ERC20Token ) -> Option<f64> {
        self.read(|ctx| ctx.pool_manager.get_token_price(token))
    }

    /// Get the v2 pool for the given tokens, token order does not matter
    pub fn get_v2_pool(&self, chain: u64, token0: Address, token1: Address) -> Option<UniswapV2Pool> {
        self.read(|ctx| ctx.pool_manager.get_v2_pool(chain, token0, token1))
    }

    /// Get the v3 pool for the given tokens, token order does not matter
    pub fn get_v3_pool(&self, chain: u64, fee: u32, token0: Address, token1: Address) -> Option<UniswapV3Pool> {
        self.read(|ctx| ctx.pool_manager.get_v3_pool(chain, fee, token0, token1))
    }

    pub fn add_v2_pools(&self, pools: Vec<UniswapV2Pool>) {
        self.write(|ctx| ctx.pool_manager.add_v2_pools(pools));
    }

    pub fn add_v3_pools(&self, pools: Vec<UniswapV3Pool>) {
        self.write(|ctx| ctx.pool_manager.add_v3_pools(pools));
    }

    /// Get all v3 pools that include the given token and [FEE_TIERS]
    pub fn get_v3_pools(&self, token: ERC20Token) -> Vec<UniswapV3Pool> {
        let base_tokens = ERC20Token::base_tokens(token.chain_id);
        let mut pools = Vec::new();

        for base_token in base_tokens {
            if base_token.address == token.address {
                continue;
            }

            for fee in FEE_TIERS {
                if let Some(pool) = self.get_v3_pool(token.chain_id, fee, base_token.address, token.address) {
                    pools.push(pool);
                }
            }
        }
        pools
    }

    /// Get all v2 pools for the given pair
    pub fn get_v2_pools(&self, token: ERC20Token) -> Vec<UniswapV2Pool> {
        let base_tokens = ERC20Token::base_tokens(token.chain_id);
        let mut pools = Vec::new();

        for base_token in base_tokens {
            if base_token.address == token.address {
                continue;
            }

            if let Some(pool) = self.get_v2_pool(token.chain_id, base_token.address, token.address) {
                pools.push(pool);
            }
        }
        pools
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

    pub pool_manager: PoolStateManagerHandle,

    pub pool_data_syncing: bool,

}

impl ZeusContext {
    pub fn new() -> Self {
        let mut providers = RpcProviders::default();
        if let Ok(loaded_providers) = RpcProviders::load_from_file() {
            providers.rpc = loaded_providers.rpc;
        }

        let profile_exists = Profile::exists().expect("Failed to read data directory");
        let rpc = providers.get(1).expect("Failed to find provider");

        let db = match db::ZeusDB::load_from_file() {
            Ok(db) => db,
            Err(e) => {
                tracing::error!("Failed to load db: {:?}", e);
                let mut db = db::ZeusDB::default();
                db.load_default_currencies().unwrap();
                db
            }
        };

        let pool_dir = pool_data_dir().unwrap().exists();
        let mut pool_manager = PoolStateManagerHandle::default();
        if pool_dir {
            let dir = pool_data_dir().unwrap();
            let data = std::fs::read(dir).unwrap();
            let manager = PoolStateManagerHandle::from_slice(&data).unwrap();
            pool_manager = manager;
        }

        Self {
            providers,
            rpc,
            chain: ChainId::new(1).unwrap(),
            profile: Profile::default(),
            profile_exists,
            logged_in: false,
            db,
            pool_manager,
            pool_data_syncing: true,
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