pub mod across;


#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Dapp {
    Across,
    Uniswap
}

impl Dapp {
    pub fn is_across(&self) -> bool {
        matches!(self, Self::Across)
    }

    pub fn is_uniswap(&self) -> bool {
        matches!(self, Self::Uniswap)
    }
}