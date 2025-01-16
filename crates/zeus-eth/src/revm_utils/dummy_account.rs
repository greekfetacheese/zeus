use crate::defi::currency::erc20::ERC20Token;
use alloy_primitives::{keccak256, Address, U256};
use alloy_signer_local::PrivateKeySigner;
use revm::primitives::{AccountInfo, Bytecode, B256};

use alloy_contract::private::Ethereum;
use alloy_provider::Provider;
use alloy_transport::Transport;

use super::{
    fork_db::fork_factory::ForkFactory,
    utils::new_evm,
    simulate::erc20_balance
};

#[derive(Clone, Debug)]
pub enum AccountType {
    /// Externally Owned Account
    EOA,

    /// An Ethereum Smart Contract
    Contract(Bytecode),
}

/// Represents a dummy account we want to insert into the fork enviroment
#[derive(Clone, Debug)]
pub struct DummyAccount {
    pub account_type: AccountType,
    pub balance: U256,
    pub address: Address,
}

impl DummyAccount {
    pub fn new(account_type: AccountType, balance: U256) -> Self {
        Self {
            account_type,
            balance,
            address: PrivateKeySigner::random().address(),
        }
    }

    /// This function will try to find the storage slot of a token
    pub fn find_balance_slot<T, P>(
        &self,
        fork_factory: &mut ForkFactory<T, P>,
        token: ERC20Token,
        amount: U256,
    ) -> Result<Option<U256>, anyhow::Error>
    where
        T: Transport + Clone + Unpin,
        P: Provider<T, Ethereum> + Clone + 'static + Unpin,
    {
        if amount == U256::ZERO {
            return Ok(Some(U256::ZERO))
        }
        
        let mut balance_slot = None;
        let slot_range = 0..200;

        // keep the orignal fork factory intact
        let mut cloned_fork_factory = fork_factory.clone();

        for slot in slot_range {
            let slot = U256::from(slot);
            self.insert_with_slot(
                &mut cloned_fork_factory,
                slot,
                token.address.clone(),
                amount,
            )?;

            let db = cloned_fork_factory.new_sandbox_fork();
            let mut evm = new_evm(db, None);
            let balance = erc20_balance(&mut evm, token.clone(), self.address.clone())?;

            if balance > U256::ZERO {
                balance_slot = Some(slot);
                break;
            }
        }
        Ok(balance_slot)
    }

    /// Insert this dummy account into the fork enviroment
    ///
    /// If you don't know the storage slot of the token you want to fund the account with, use this function
    pub fn insert<T, P>(
        &self,
        fork_factory: &mut ForkFactory<T, P>,
        token: ERC20Token,
        amount: U256,
    ) -> Result<(), anyhow::Error>
    where
        T: Transport + Clone + Unpin,
        P: Provider<T, Ethereum> + Clone + 'static + Unpin,
    {
        let slot = self.find_balance_slot(fork_factory, token.clone(), amount)?;
        if let Some(slot) = slot {
            self.insert_with_slot(fork_factory, slot, token.address, amount)
        } else {
            Err(anyhow::anyhow!(
                "Balance Storage Slot not found for: {}",
                token.symbol
            ))
        }
    }

    /// Insert this dummy account into the fork enviroment
    ///
    /// If you know the storage slot of the token you want to fund the account with, use this function
    pub fn insert_with_slot<T, P>(
        &self,
        fork_factory: &mut ForkFactory<T, P>,
        slot: U256,
        token: Address,
        amount: U256,
    ) -> Result<(), anyhow::Error>
    where
        T: Transport + Clone + Unpin,
        P: Provider<T, Ethereum> + Clone + 'static + Unpin,
    {
        let code = match &self.account_type {
            AccountType::EOA => Bytecode::default(),
            AccountType::Contract(code) => code.clone(),
        };

        let eth_balance = self.balance.clone();
        let address = self.address.clone();

        let account_info = AccountInfo {
            balance: eth_balance,
            nonce: 0,
            code_hash: B256::default(),
            code: Some(code),
        };

        fork_factory.insert_account_info(address.clone(), account_info);

        let addr_padded = pad_left(address.to_vec(), 32);
        let slot = slot.to_be_bytes_vec();

        let data = [&addr_padded, &slot]
            .iter()
            .flat_map(|x| x.iter().copied())
            .collect::<Vec<u8>>();
        let slot_hash = keccak256(&data);
        let slot: U256 =
            U256::from_be_bytes(slot_hash.try_into().expect("Slot Hash must be 32 bytes"));

        if let Err(e) = fork_factory.insert_account_storage(token, slot, amount) {
            return Err(anyhow::anyhow!("Failed to insert account storage: {}", e));
        }

        Ok(())
    }
}

fn pad_left(vec: Vec<u8>, full_len: usize) -> Vec<u8> {
    let mut padded = vec![0u8; full_len - vec.len()];
    padded.extend(vec);
    padded
}