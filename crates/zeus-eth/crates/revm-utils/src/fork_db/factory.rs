use std::sync::mpsc::channel as oneshot_channel;

use alloy_contract::private::{Ethereum, Provider};

use super::ForkDB;
use super::backend::{BackendFetchRequest, GlobalBackend};
use super::error::DatabaseResult;

use alloy_rpc_types::eth::BlockId;
use futures::channel::mpsc::{Sender, channel};
use revm::database::InMemoryDB;
use revm::primitives::{Address, U256};
use revm::state::AccountInfo;

/// Type that setups up backend and clients to talk to backend
/// each client is an own evm instance but we cache request results
/// to avoid excessive rpc calls
#[derive(Clone, Debug)]
pub struct ForkFactory<P> {
   backend: Sender<BackendFetchRequest>,
   initial_db: InMemoryDB,
   #[allow(dead_code)]
   provider: P,
}

impl<P> ForkFactory<P>
where
   P: Provider<(), Ethereum> + Clone + 'static + Unpin,
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
   fn new(provider: P, initial_db: InMemoryDB, fork_block: Option<BlockId>) -> (Self, GlobalBackend<P>) {
      let (backend, backend_rx) = channel(1);
      let handler = GlobalBackend::new(backend_rx, fork_block, provider.clone(), initial_db.clone());
      (
         Self {
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
   pub fn new_sandbox_factory(provider: P, initial_db: InMemoryDB, fork_block: Option<BlockId>) -> Self {
      let (shared, handler) = Self::new(provider, initial_db, fork_block);

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

   #[allow(dead_code)]
   // Insert storage into local db
   pub fn insert_account_storage(&mut self, address: Address, slot: U256, value: U256) -> DatabaseResult<()> {
      if self.initial_db.cache.accounts.get(&address).is_none() {
         // set basic info as its missing
         let info = match self.do_get_basic(address) {
            Ok(i) => i,
            Err(e) => return Err(e),
         };

         // keep record of fetched acc basic info
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

   #[allow(dead_code)]
   // Insert account basic info into local db
   pub fn insert_account_info(&mut self, address: Address, info: AccountInfo) {
      self.initial_db.insert_account_info(address, info);
   }
}
