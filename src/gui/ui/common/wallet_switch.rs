//! Changing the active wallet, and everything that has to follow it.

use crate::core::{WalletInfo, ZeusContext};
use crate::gui::SHARED_GUI;

/// Switch the active wallet and refresh everything that depends on it.
///
/// The single implementation of "the wallet changed": the account panel's selector, the open token
/// picker and the send and shield defaults all have to follow together, and a second copy of that
/// list drifts apart. Anything that changes `ctx.current_wallet` from a click goes through here.
pub fn switch_wallet(ctx: &mut ZeusContext, wallet: WalletInfo) {
   ctx.current_wallet = wallet;

   // Update the state on wallet change
   crate::runtime::rt().spawn(async move {
      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
      let current_wallet = ctx.current_wallet_info();
      let privacy_mode = ctx.read(|ctx| ctx.privacy_mode);
      let owner = current_wallet.address;
      let chain_id = ctx.chain().id();

      SHARED_GUI.write(|gui| {
         // The panel's selector follows the wallet the rest of the app is on, and its body reads
         // the same `wallet_info`.
         gui.account_panel.set_current_wallet(current_wallet);

         // The token picker's lists are keyed on (privacy mode, chain, owner), so a new owner has
         // to reload them.
         if gui.token_selection.is_open() {
            gui.token_selection.process_currencies(privacy_mode, chain_id, owner);
         }

         // The send view is paired with a wallet too: a token — or an NFT — picked for the
         // previous one cannot be sent from this one. Resetting to the new wallet's default
         // clears the NFT selection along with the fungible one.
         if gui.send_crypto.is_open() {
            gui.send_crypto.default_currency(privacy_mode, chain_id);
         }

         // The shield view holds a wallet's asset in the same way, and is reset the way a chain
         // switch resets it: unconditionally, so a token or NFT picked for the previous wallet is
         // gone before the view is next opened rather than only when it happens to be open now.
         gui.shield_ui.default_currency(chain_id);

         gui.request_repaint();
      });
   });
}
