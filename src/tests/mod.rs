mod connector;
mod railgun;
pub mod stateview;
mod swap;
//mod zeus_router;

#[cfg(test)]
pub fn unlock_ctx() -> crate::core::ZeusCtx {
   use crate::core::Vault;
   use crate::core::WalletState;
   use crate::core::ZeusCtx;
   use ncrypt_me::{Credentials, secure_types::SecureString};

   let ctx = ZeusCtx::new();

   let credentials = Credentials::new(
      SecureString::from("dev"),
      SecureString::from("dev"),
      SecureString::from("dev"),
   );

   let mut vault = Vault::default();
   vault.set_credentials(credentials);

   let data = vault.decrypt(None).unwrap();
   vault.load(data).unwrap();

   let key = vault.wallet_state_key().unwrap();

   let (state, _) = WalletState::load_or_migrate(&key, None).unwrap();

   ctx.set_vault(vault);
   ctx.set_wallet_state(state);
   ctx.build_wallet_info_cache();
   ctx.load_currency_db();
   ctx.load_pool_manager();
   ctx.load_zeus_client();

   ctx
}
