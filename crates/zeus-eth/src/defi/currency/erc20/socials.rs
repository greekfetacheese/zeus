use alloy_primitives::Address;
use serde::{Deserialize, Serialize};

/// A social link for a token
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SocialLink {
    CoinGecko(String),
    GitHub(String),
    Telegram(String),
    X(String),
    None,
}

impl Default for SocialLink {
    fn default() -> Self {
        SocialLink::None
    }
}

impl SocialLink {
    pub fn to_string(&self) -> String {
        match self {
            SocialLink::CoinGecko(url) => url.clone(),
            SocialLink::GitHub(url) => url.clone(),
            SocialLink::Telegram(url) => url.clone(),
            SocialLink::X(url) => url.clone(),
            SocialLink::None => "".to_string(),
        }
    }

    pub fn from_str(url: &str) -> Self {
        if url.starts_with("https://www.coingecko.com/") {
            return SocialLink::CoinGecko(url.to_string());
        } else if url.starts_with("https://github.com/") {
            return SocialLink::GitHub(url.to_string());
        } else if url.starts_with("https://t.me/") {
            return SocialLink::Telegram(url.to_string());
        } else if url.starts_with("https://x.com/") {
            return SocialLink::X(url.to_string());
        } else {
            return SocialLink::None;
        }
    }
}

/// A tag to categorize an ERC20 token
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TokenTag {
    USDStableCoin,
    EURStableCoin,
    LiquidStaking,
    LiquidStakedETH,
    DeFi,
    NFT,
    Metaverse,
    Meme,
    Oracle,
    DEX,
    YieldFarming,
    GameFi,
    AI,
    RWA,
    None,
}

impl Default for TokenTag {
    fn default() -> Self {
        TokenTag::None
    }
}

impl TokenTag {
    pub fn to_str(&self) -> &'static str {
        match self {
            TokenTag::USDStableCoin => "USD Stablecoin",
            TokenTag::EURStableCoin => "EUR Stablecoin",
            TokenTag::LiquidStaking => "Liquid Staking",
            TokenTag::LiquidStakedETH => "Liquid StakedETH",
            TokenTag::DeFi => "DeFi",
            TokenTag::NFT => "NFT",
            TokenTag::Metaverse => "Metaverse",
            TokenTag::Meme => "Meme",
            TokenTag::Oracle => "Oracle",
            TokenTag::DEX => "DEX",
            TokenTag::YieldFarming => "Yield Farming",
            TokenTag::GameFi => "GameFi",
            TokenTag::AI => "AI",
            TokenTag::RWA => "RWA",
            TokenTag::None => "None",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "USD Stablecoin" => TokenTag::USDStableCoin,
            "EUR Stablecoin" => TokenTag::EURStableCoin,
            "Liquid Staking" => TokenTag::LiquidStaking,
            "Liquid StakedETH" => TokenTag::LiquidStakedETH,
            "DeFi" => TokenTag::DeFi,
            "NFT" => TokenTag::NFT,
            "Metaverse" => TokenTag::Metaverse,
            "Meme" => TokenTag::Meme,
            "Oracle" => TokenTag::Oracle,
            "DEX" => TokenTag::DEX,
            "Yield Farming" => TokenTag::YieldFarming,
            "GameFi" => TokenTag::GameFi,
            "AI" => TokenTag::AI,
            "RWA" => TokenTag::RWA,
            _ => TokenTag::None,
        }
    }

    pub fn to_string(&self) -> String {
        self.to_str().to_string()
    }

    pub fn is_none(&self) -> bool {
        matches!(self, TokenTag::None)
    }
}

/// Social information about an ERC20 token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSocials {
    pub token: Address,
    pub chain_id: u64,
    pub website: String,
    pub description: String,
    pub tags: Vec<TokenTag>,
    pub links: Vec<SocialLink>,
}

impl Default for TokenSocials {
    fn default() -> Self {
        Self {
            token: Address::ZERO,
            chain_id: 1,
            website: String::new(),
            description: String::new(),
            tags: Vec::new(),
            links: Vec::new(),
        }
    }
}

impl TokenSocials {
    pub fn new(
        token: Address,
        chain_id: u64,
        website: String,
        description: String,
        tags: Vec<TokenTag>,
        links: Vec<SocialLink>,
    ) -> Self {
        Self {
            token,
            chain_id,
            website,
            description,
            tags,
            links,
        }
    }

    pub fn add_link(&mut self, link: SocialLink) {
        self.links.push(link);
    }

    pub fn remove_link(&mut self, link: SocialLink) {
        self.links.retain(|l| *l != link);
    }

    pub fn add_tag(&mut self, tag: TokenTag) {
        self.tags.push(tag);
    }

    pub fn remove_tag(&mut self, tag: TokenTag) {
        self.tags.retain(|t| *t != tag);
    }
}
