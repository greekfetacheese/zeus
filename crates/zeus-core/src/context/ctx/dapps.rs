use super::super::{ZeusContext, ZeusCtx};
use anyhow::anyhow;
use zeus_eth::{alloy_primitives::Address, alloy_provider::Provider};

impl ZeusCtx {
   pub fn get_connected_dapps(&self) -> Vec<String> {
      self.read(|ctx| ctx.connected_dapps.connected_dapps())
   }

   pub fn connect_dapp(&self, dapp: String) {
      self.write(|ctx| {
         ctx.connected_dapps.connect_dapp(dapp);
      });
   }

   pub fn disconnect_dapp(&self, dapp: &str) {
      self.write(|ctx| {
         ctx.connected_dapps.disconnect_dapp(dapp);
      });
   }

   pub fn disconnect_all_dapps(&self) {
      self.write(|ctx| {
         ctx.connected_dapps.disconnect_all();
      });
   }

   pub fn is_dapp_connected(&self, dapp: &str) -> bool {
      self.read(|ctx| ctx.connected_dapps.is_connected(dapp))
   }

   /// The account Zeus gave `origin`, if the app has connected before.
   pub fn dapp_account(&self, origin: &str) -> Option<Address> {
      self.read_wallet_state(|ws| ws.dapp_accounts.get(origin))
   }

   /// Every account `origin` has been given, oldest first.
   pub fn dapp_seen_accounts(&self, origin: &str) -> Vec<Address> {
      self.read_wallet_state(|ws| ws.dapp_accounts.seen(origin).to_vec())
   }

   /// Remember `address` as the account exposed to `origin` and persist it.
   ///
   /// Disconnecting an app does not clear this, and the accounts the app was
   /// given before are kept too, so reconnecting offers the same account and the
   /// prompt can still mark the earlier ones.
   pub fn set_dapp_account(&self, origin: &str, address: Address) -> Result<(), anyhow::Error> {
      self.write_wallet_state(|ws| ws.dapp_accounts.record(origin, address));
      self.save_wallet_state()
   }

   /// Derive a fresh child wallet to serve as a new app's dedicated account.
   ///
   /// The wallet keeps the usual auto-generated name (e.g. `Wallet 5`) so
   /// per-app accounts stay unobtrusive in the wallet list. The vault write runs
   /// Argon2, so callers must keep this off the GUI thread.
   pub fn create_app_account(&self) -> Result<Address, anyhow::Error> {
      let mut new_vault = self.get_vault();
      let address = new_vault.derive_child_wallet(String::new())?;
      self.encrypt_and_save_vault(Some(new_vault.clone()), None)?;
      self.set_vault(new_vault);
      self.build_wallet_info_cache();
      Ok(address)
   }

   pub fn should_check_delegated_wallet_status(&self, chain: u64, account: Address) -> bool {
      self.read(|ctx| ctx.delegated_wallets.should_check(chain, account))
   }

   pub async fn check_delegated_wallet_status(
      &self,
      chain: u64,
      account: Address,
   ) -> Result<(), anyhow::Error> {
      if self.is_chain_disabled(chain) {
         return Ok(());
      }

      let client = self.get_client_manager();
      let code = client
         .request(chain, |client| async move {
            client.get_code_at(account).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      if code.is_empty() {
         self.write(|ctx| {
            ctx.delegated_wallets.remove(chain, account);
         });
         return Ok(());
      }

      let addr_slice = &code[3..];
      let delegated_address = Address::from_slice(addr_slice);

      self.write(|ctx| {
         ctx.delegated_wallets.add(chain, account, delegated_address);
      });

      Ok(())
   }
}

impl ZeusContext {
   pub fn connected_dapps(&self) -> Vec<String> {
      self.connected_dapps.connected_dapps()
   }

   /// The account an app was connected with, if it has connected before.
   pub fn dapp_account(&self, origin: &str) -> Option<Address> {
      self.wallet_state.read(|ws| ws.dapp_accounts.get(origin))
   }

   pub fn connect_dapp(&mut self, dapp: String) {
      self.connected_dapps.connect_dapp(dapp);
   }

   pub fn disconnect_dapp(&mut self, dapp: &str) {
      self.connected_dapps.disconnect_dapp(dapp);
   }

   pub fn disconnect_all_dapps(&mut self) {
      self.connected_dapps.disconnect_all();
   }
}
