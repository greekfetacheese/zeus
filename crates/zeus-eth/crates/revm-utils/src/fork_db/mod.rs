use std::sync::mpsc::channel as oneshot_channel;

use futures::channel::mpsc::Sender;
use revm::primitives::{Address, B256, HashMap, KECCAK_EMPTY, U256};

use revm::database::InMemoryDB;
use revm::database_interface::{Database, DatabaseCommit, DatabaseRef};
use revm::state::{Account, AccountInfo, Bytecode};

use backend::BackendFetchRequest;
use error::{DatabaseError, DatabaseResult};

pub mod backend;
pub mod error;
pub mod factory;

pub use factory::ForkFactory;

#[derive(Clone, Debug)]
pub struct ForkDB {
   // used to make calls for missing data
   backend: Sender<BackendFetchRequest>,
   pub db: InMemoryDB,
}

impl ForkDB {
   pub fn new(backend: Sender<BackendFetchRequest>, db: InMemoryDB) -> Self {
      Self { backend, db }
   }

   fn do_get_basic(&self, address: Address) -> DatabaseResult<Option<AccountInfo>> {
      tokio::task::block_in_place(|| {
         let (sender, rx) = oneshot_channel();
         let req = BackendFetchRequest::Basic(address, sender);
         self.backend.clone().try_send(req)?;
         rx.recv()?.map(Some)
      })
   }

   fn do_get_storage(&self, address: Address, index: U256) -> DatabaseResult<U256> {
      tokio::task::block_in_place(|| {
         let (sender, rx) = oneshot_channel();
         let req = BackendFetchRequest::Storage(address, index, sender);
         self.backend.clone().try_send(req)?;
         rx.recv()?
      })
   }

   fn do_get_block_hash(&self, number: u64) -> DatabaseResult<B256> {
      tokio::task::block_in_place(|| {
         let (sender, rx) = oneshot_channel();
         let req = BackendFetchRequest::BlockHash(number, sender);
         self.backend.clone().try_send(req)?;
         rx.recv()?
      })
   }
}

impl Database for ForkDB {
   type Error = DatabaseError;

   fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
      // found locally, return it
      match self.db.cache.accounts.get(&address) {
         // basic info is already in db
         Some(account) => Ok(Some(account.info.clone())),
         None => {
            // basic info is not in db, make rpc call to fetch it
            let info = match self.do_get_basic(address) {
               Ok(i) => i,
               Err(e) => return Err(e),
            };

            // keep record of fetched acc basic info
            if info.is_some() {
               self.db.insert_account_info(address, info.clone().unwrap());
            }

            Ok(info)
         }
      }
   }

   fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
      // found locally, return it
      if let Some(account) = self.db.cache.accounts.get(&address) {
         if let Some(entry) = account.storage.get(&index) {
            // account storage exists at slot
            return Ok(*entry);
         }
      }

      // get account info
      let acc_info = match self.do_get_basic(address) {
         Ok(a) => a,
         Err(e) => return Err(e),
      };

      if let Some(a) = acc_info {
         self.db.insert_account_info(address, a);
      }

      // make rpc call to fetch storage
      let storage_val = match self.do_get_storage(address, index) {
         Ok(i) => i,
         Err(e) => return Err(e),
      };

      // keep record of fetched storage (can unwrap safely as cacheDB always returns true)
      self
         .db
         .insert_account_storage(address, index, storage_val)
         .unwrap();

      Ok(storage_val)
   }

   fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
      match self.db.cache.block_hashes.get(&U256::from(number)) {
         // found locally, return it
         Some(hash) => Ok(*hash),
         None => {
            // rpc call to fetch block hash
            let block_hash = match self.do_get_block_hash(number) {
               Ok(i) => i,
               Err(e) => return Err(e),
            };

            // insert fetched block hash into db
            self
               .db
               .cache
               .block_hashes
               .insert(U256::from(number), block_hash);

            Ok(block_hash)
         }
      }
   }

   /// Get account code by its hash
   fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
      match self.db.code_by_hash(code_hash) {
         Ok(code) => Ok(code),
         Err(_) => {
            // should alr be loaded
            Err(DatabaseError::MissingCode(code_hash))
         }
      }
   }
}

impl DatabaseRef for ForkDB {
   type Error = DatabaseError;

   fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
      match self.db.cache.accounts.get(&address) {
         Some(account) => Ok(Some(account.info.clone())),
         None => {
            // state doesnt exist so fetch it
            self.do_get_basic(address)
         }
      }
   }

   fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
      match self.db.cache.accounts.get(&address) {
         Some(account) => match account.storage.get(&index) {
            Some(entry) => Ok(*entry),
            None => {
               // state doesnt exist so fetch it
               match self.do_get_storage(address, index) {
                  Ok(storage) => Ok(storage),
                  Err(e) => Err(e),
               }
            }
         },
         None => {
            // state doesnt exist so fetch it
            match self.do_get_storage(address, index) {
               Ok(storage) => Ok(storage),
               Err(e) => Err(e),
            }
         }
      }
   }

   fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
      if number > u64::MAX {
         return Ok(KECCAK_EMPTY);
      }
      self.do_get_block_hash(number)
   }

   /// Get account code by its hash
   fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
      match self.db.clone().code_by_hash(code_hash) {
         Ok(code) => Ok(code),
         Err(_) => {
            // should alr be loaded
            Err(DatabaseError::MissingCode(code_hash))
         }
      }
   }
}

impl DatabaseCommit for ForkDB {
   fn commit(&mut self, changes: HashMap<Address, Account>) {
      self.db.commit(changes)
   }
}
