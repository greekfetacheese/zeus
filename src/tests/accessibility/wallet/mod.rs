//! The wallet area: the Wallets page and the dialogs it opens, plus the first-run recovery flow
//! the page is shown behind.

mod delete;
mod export;
mod page;

use super::harness::frame_nodes;
use crate::core::ZeusCtx;
use crate::gui::GUI;
use egui::accesskit::{Node, NodeId};
use egui::{Context, Event};

/// Draw the central panel and then the announcer, which is the order the app draws them in and the
/// only way a page's sentence is said: a page cannot announce from inside its own draw.
pub(super) fn render_wallet_page(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, events, |ui| {
      zeus.write(|zeus| crate::gui::ui::panels::central_panel::show(gui, zeus, ui));
      gui.show_announcer(ui);
   })
}
