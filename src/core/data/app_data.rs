use anyhow::anyhow;
use zeus_eth::ChainId;
use serde::{ Deserialize, Serialize };
use crate::core::user::{wallet::Wallet, Profile};
use crate::core::utils::data_dir;
use crate::assets::icons::Icons;
use std::sync::{ Arc, RwLock };
use lazy_static::lazy_static;
use crate::core::utils::{ HttpClient, get_http_client };
use zeus_eth::alloy_primitives::Address;

pub const PROVIDERS_FILE: &str = "rpc_providers.json";

lazy_static! {
    pub static ref APP_DATA: Arc<RwLock<AppData>> = Arc::new(RwLock::new(AppData::new()));
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rpc {
    pub url: String,

    pub chain_id: u64,

    /// False if the rpc is added by the user from the GUI
    pub default: bool,
}

impl Rpc {
    pub fn new(url: impl Into<String>, chain_id: u64, default: bool) -> Self {
        Self {
            url: url.into(),
            chain_id,
            default,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcProviders {
    pub rpc: Vec<Rpc>,
}

impl RpcProviders {
    pub fn load_from_file() -> Result<Self, anyhow::Error> {
        let dir = data_dir()?.join(PROVIDERS_FILE);
        let data = std::fs::read(dir)?;
        let providers = serde_json::from_slice(&data)?;
        Ok(providers)
    }

    pub fn save_to_file(&self) -> Result<(), anyhow::Error> {
        let providers = serde_json::to_vec(&self)?;
        let dir = data_dir()?.join(PROVIDERS_FILE);
        std::fs::write(dir, providers)?;
        Ok(())
    }

    /// Get an rpc provider based on the chain_id
    pub fn get(&self, chain_id: u64) -> Result<Rpc, anyhow::Error> {
        let rpc_opt = self.rpc.iter().find(|rpc| rpc.chain_id == chain_id);
        if let Some(rpc) = rpc_opt {
            Ok(rpc.clone())
        } else {
            Err(anyhow!("Rpc for chain id {} not found", chain_id))
        }
    }
}

impl Default for RpcProviders {
    fn default() -> Self {
        let eth = Rpc::new("https://eth.merkle.io", 1, true);
        let base = Rpc::new("https://base.llamarpc.com", 8453, true);
        let op = Rpc::new("https://optimism.llamarpc.com", 10, true);
        let arbitrum = Rpc::new("https://arbitrum.llamarpc.com", 42161, true);
        let bsc = Rpc::new("https://binance.llamarpc.com", 56, true);

        RpcProviders { rpc: vec![eth, base, op, arbitrum, bsc] }
    }
}

/// Main data and settings loaded by the app
pub struct AppData {
    /// List of rpc providers
    pub rpc_providers: RpcProviders,

    /// The current selected Rpc Provider from the GUI
    pub current_rpc: Rpc,

    /// The current chain id
    pub chain_id: ChainId,

    pub supported_chains: Vec<ChainId>,

    /// The current profile
    pub profile: Profile,

    pub profile_exists: bool,

    pub logged_in: bool,

    /// Does Zeus runs for the first time?
    pub first_run: bool,

    pub icons: Option<Arc<Icons>>
}

impl AppData {
    pub fn new() -> Self {
        let mut rpc_providers = RpcProviders::default();
        if let Ok(loaded_providers) = RpcProviders::load_from_file() {
            rpc_providers.rpc = loaded_providers.rpc;
        }

        // This should not panic
        let profile_exists = Profile::exists().expect("Failed to read data directory");
        let current_rpc = rpc_providers.get(1).expect("Failed to find provider");

        Self {
            rpc_providers,
            current_rpc,
            chain_id: ChainId::new(1).unwrap(),
            supported_chains: ChainId::supported_chains(),
            profile: Profile::default(),
            profile_exists,
            logged_in: false,
            first_run: profile_exists,
            icons: None
        }
    }

    pub fn get_client(&self) -> Result<HttpClient, anyhow::Error> {
        let rpc = self.rpc_providers.get(self.chain_id.id())?;
        let client = get_http_client(&rpc.url)?;
        Ok(client)
    }

    /// Get the current wallet selected from the GUI
    pub fn get_wallet(&self) -> &Wallet {
        &self.profile.current_wallet
    }

    /// Get the address of the current wallet
    pub fn get_address(&self) -> Address {
        self.profile.wallet_address()
    }
}