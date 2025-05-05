use std::sync::mpsc::channel as oneshot_channel;

use super::ForkDB;
use super::backend::{BackendFetchRequest, GlobalBackend};
use super::error::DatabaseResult;
use crate::{AccountType, DummyAccount};
use alloy_contract::private::{Ethereum, Provider};
use alloy_primitives::utils::keccak256;
use alloy_rpc_types::eth::BlockId;
use futures::channel::mpsc::{Sender, channel};
use revm::database::InMemoryDB;
use revm::primitives::{Address, B256, U256};
use revm::state::AccountInfo;
use revm::state::Bytecode;

use crate::new_evm;
use crate::simulate::erc20_balance;

/// Type that setups up backend and clients to talk to backend
/// each client is an own evm instance but we cache request results
/// to avoid excessive rpc calls
#[derive(Clone, Debug)]
pub struct ForkFactory<P> {
   pub chain_id: u64,
   backend: Sender<BackendFetchRequest>,
   initial_db: InMemoryDB,
   #[allow(dead_code)]
   provider: P,
}

impl<P> ForkFactory<P>
where
   P: Provider<Ethereum> + Clone + 'static + Unpin,
{
   // Create a new `ForkFactory` instance
   //
   // Arguments:
   // * `provider`: Websocket client used for fetching missing state
   // * `initial_db`: Database with initial state
   // * `fork_block`: Block to fork from when making rpc calls
   //
   // Returns:
   // `(ForkFactory, GlobalBackend)`: ForkFactory instance and the GlobalBackend it talks to
   fn new(provider: P, chain_id: u64, initial_db: InMemoryDB, fork_block: Option<BlockId>) -> (Self, GlobalBackend<P>) {
      let (backend, backend_rx) = channel(1);
      let handler = GlobalBackend::new(backend_rx, fork_block, provider.clone(), initial_db.clone());
      (
         Self {
            chain_id,
            backend,
            initial_db,
            provider,
         },
         handler,
      )
   }

   // Used locally in `insert_account_storage` to fetch accoutn info if account does not exist
   fn do_get_basic(&self, address: Address) -> DatabaseResult<Option<AccountInfo>> {
      tokio::task::block_in_place(|| {
         let (sender, rx) = oneshot_channel();
         let req = BackendFetchRequest::Basic(address, sender);
         self.backend.clone().try_send(req)?;
         rx.recv()?.map(Some)
      })
   }

   // Create a new sandbox environment with backend running on own thread
   pub fn new_sandbox_factory(
      provider: P,
      chain_id: u64,
      initial_db: Option<InMemoryDB>,
      fork_block: Option<BlockId>,
   ) -> Self {
      let initial_db = initial_db.unwrap_or_else(|| InMemoryDB::default());
      let (shared, handler) = Self::new(provider, chain_id, initial_db, fork_block);

      // spawn a light-weight thread with a thread-local async runtime just for
      // sending and receiving data from the remote client
      let _ = std::thread::Builder::new()
         .name("fork-backend-thread".to_string())
         .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
               .enable_all()
               .build()
               .expect("failed to create fork-backend-thread tokio runtime");

            rt.block_on(async move { handler.await });
         })
         .expect("failed to spawn backendhandler thread");

      shared
   }

   /// Creates new ForkDB that fallsback on this `ForkFactory` instance
   pub fn new_sandbox_fork(&self) -> ForkDB {
      ForkDB::new(self.backend.clone(), self.initial_db.clone())
   }

   /// Insert storage into local db
   pub fn insert_account_storage(&mut self, address: Address, slot: U256, value: U256) -> DatabaseResult<()> {
      if self.initial_db.cache.accounts.get(&address).is_none() {
         // set basic info as its missing
         let info = match self.do_get_basic(address) {
            Ok(i) => i,
            Err(e) => return Err(e),
         };

         if info.is_some() {
            self.initial_db.insert_account_info(address, info.unwrap());
         }
      }
      self
         .initial_db
         .insert_account_storage(address, slot, value)
         .unwrap();

      Ok(())
   }

   /// Insert account basic info into local db
   pub fn insert_account_info(&mut self, address: Address, info: AccountInfo) {
      self.initial_db.insert_account_info(address, info);
   }

   /// Insert this dummy account into the fork enviroment
   pub fn insert_dummy_account(&mut self, account: DummyAccount) {
      let code = match account.account_type {
         AccountType::EOA => Bytecode::default(),
         AccountType::Contract(code) => code.clone(),
      };

      let eth_balance = account.balance;
      let address = account.address;

      self.insert_account_info(
         address,
         AccountInfo {
            balance: eth_balance,
            nonce: 0,
            code_hash: B256::default(),
            code: Some(code),
         },
      );
   }

   /// Give this account the given amount of ETH
   pub fn give_eth(&mut self, account: Address, amount: U256) {
      if let Some(account) = self.initial_db.cache.accounts.get_mut(&account) {
         account.info.balance += amount;
      }
   }

   /// Set the ETH balance of the given account
   pub fn set_eth_balance(&mut self, account: Address, amount: U256) {
      if let Some(account) = self.initial_db.cache.accounts.get_mut(&account) {
         account.info.balance = amount;
      }
   }

   /// Give this account the given amount of ERC20 token
   pub fn give_token(&mut self, account: Address, token: Address, amount: U256) -> Result<(), anyhow::Error> {
      let slot = self.find_balance_slot(account, token, amount)?;
      if let Some(slot) = slot {
         self.give_token_with_slot(account, token, slot, amount)
      } else {
         Err(anyhow::anyhow!(
            "Balance Storage Slot not found for: {}",
            token
         ))
      }
   }

   /// Give this account the given amount of ERC20 token
   pub fn give_token_with_slot(
      &mut self,
      account: Address,
      token: Address,
      slot: U256,
      amount: U256,
   ) -> Result<(), anyhow::Error> {
      let addr_padded = pad_left(account.to_vec(), 32);
      let slot = slot.to_be_bytes_vec();

      let data = [&addr_padded, &slot]
         .iter()
         .flat_map(|x| x.iter().copied())
         .collect::<Vec<u8>>();
      let slot_hash = keccak256(&data);
      let slot: U256 = U256::from_be_bytes(slot_hash.try_into().expect("Slot Hash must be 32 bytes"));

      if let Err(e) = self.insert_account_storage(token, slot, amount) {
         return Err(anyhow::anyhow!("Failed to insert account storage: {}", e));
      }
      Ok(())
   }

   /// This function will try to find the balance storage slot of a token
   pub fn find_balance_slot(
      &mut self,
      owner: Address,
      token: Address,
      amount: U256,
   ) -> Result<Option<U256>, anyhow::Error> {
      if amount == U256::ZERO {
         return Ok(Some(U256::ZERO));
      }

      let mut balance_slot = None;
      let slot_range = 0..200;

      // keep the orignal fork factory intact
      let mut cloned_fork_factory = self.clone();

      for slot in slot_range {
         let slot = U256::from(slot);
         cloned_fork_factory.give_token_with_slot(owner, token, slot, amount)?;

         let db = cloned_fork_factory.new_sandbox_fork();
         let mut evm = new_evm(self.chain_id.into(), None, db);
         let balance = erc20_balance(&mut evm, token, owner)?;

         if balance > U256::ZERO {
            balance_slot = Some(slot);
            break;
         }
      }
      Ok(balance_slot)
   }
}

fn pad_left(vec: Vec<u8>, full_len: usize) -> Vec<u8> {
   let mut padded = vec![0u8; full_len - vec.len()];
   padded.extend(vec);
   padded
}
