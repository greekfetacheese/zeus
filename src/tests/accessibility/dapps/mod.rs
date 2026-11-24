//! The dapp area: the connect prompt, the two Uniswap pages, the Across bridge and the Railgun
//! shield and merge windows, plus the Uniswap helpers two of those pages share.

mod across;
mod connect;
mod uniswap_pools;
mod uniswap_swap;

use super::harness::frame_nodes;
use crate::core::ZeusCtx;
use crate::gui::GUI;
use egui::Context;
use egui::accesskit::{Node, NodeId};
use zeus_eth::alloy_primitives::{Address, U256};
use zeus_eth::currency::{Currency, ERC20Token};

/// The Uniswap window as the central panel draws it, on the page it opens on.
///
/// Twice: the window fades its page in on the pass that first sees it open, and the second pass is
/// the one a reader is handed.
pub(super) fn render_uniswap_window(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for _ in 0..2 {
      nodes = frame_nodes(ctx, Vec::new(), |ui| {
         let icons = gui.icons.clone();
         let theme = gui.theme.clone();
         let uniswap = &mut gui.uniswap;
         let token_selection = &mut gui.token_selection;

         zeus.write(|zeus| uniswap.show(zeus, &theme, icons, token_selection, ui));
      });
   }

   nodes
}

/// The two currencies a seeded pool row draws: the symbols the pair cell prints.
pub(super) fn weth_usdc() -> (Currency, Currency) {
   let weth = ERC20Token::from_components(
      1,
      Address::from([0xbb; 20]),
      "WETH",
      "Wrapped Ether",
      18,
      U256::ZERO,
   );

   let usdc = ERC20Token::from_components(
      1,
      Address::from([0xcc; 20]),
      "USDC",
      "USD Coin",
      6,
      U256::ZERO,
   );

   (Currency::from(weth), Currency::from(usdc))
}
