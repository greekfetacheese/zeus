//! Changing the active chain, and everything that has to follow it.

use crate::core::ZeusContext;
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use zeus_eth::{
   currency::{Currency, NativeCurrency},
   types::ChainId,
};

/// Switch the active chain and refresh everything that depends on it.
///
/// The single implementation of "the chain changed": the selected currency, the open token
/// picker, the swap defaults and the cached wallet values all have to be reset together, and a
/// second copy of that list drifts apart. Anything that changes `ctx.chain` goes through here.
pub fn switch_chain(ctx: &mut ZeusContext, new_chain: ChainId) {
   ctx.chain = new_chain;

   RT.spawn(async move {
      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
      let owner = ctx.current_wallet_info().address;
      let privacy_mode = ctx.read(|ctx| ctx.privacy_mode);

      SHARED_GUI.write(|gui| {
         let currency: Currency = NativeCurrency::from(new_chain.id()).into();
         gui.send_crypto.set_currency(currency.clone());

         if gui.token_selection.is_open() {
            gui.token_selection.process_currencies(privacy_mode, new_chain.id(), owner);
         }

         gui.account_panel.set_current_chain(new_chain);
         gui.uniswap.swap_ui.default_currency_in(new_chain.id());
         gui.uniswap.swap_ui.default_currency_out(new_chain.id());
         gui.send_crypto.default_currency(privacy_mode, new_chain.id());
         gui.shield_ui.default_currency(new_chain.id());
         gui.wallet_ui.calc_wallet_value();
         gui.recipient_selection.calc_wallet_value();
      });
   });
}
