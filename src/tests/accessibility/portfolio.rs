use super::app::shared_gui_turn;
use super::harness::{accesskit_action, drawn_names, focused_id, frame_nodes, key_press};
use egui::accesskit::{Action, Node, NodeId};
use egui::{Context, Event, Key};

/// What a row of the portfolio says to a reader, decided away from the page that draws it.
///
/// The total is spoken once when the page is opened, and again only when the user's own Refresh
/// moves it: a rebuild nobody asked for, a wallet switch and a frame tick all leave a number that
/// is already known alone, and leaving the page makes the next arrival a new one.
#[test]
fn the_portfolio_total_is_announced_once_per_arrival() {
   use crate::gui::ui::portfolio::announcement;

   let chain = "Ethereum";

   // The rows are built off the GUI thread, so the page opens a frame or two before it has a
   // total at all.
   assert_eq!(
      announcement(None, None, false, chain),
      None,
      "a total that has not landed says nothing"
   );

   assert_eq!(
      announcement(None, Some("$1,234.56"), false, chain).as_deref(),
      Some("Portfolio value on Ethereum, $1,234.56"),
      "the first sight of a known total is what a reader hears"
   );

   assert_eq!(
      announcement(Some("$1,234.56"), Some("$1,234.56"), false, chain),
      None,
      "the same total a frame later is not a second announcement"
   );

   assert_eq!(
      announcement(Some("$1,234.56"), Some("$1,300.00"), false, chain),
      None,
      "a total that moved on its own is not the user asking about it"
   );

   assert_eq!(
      announcement(Some("$1,300.00"), Some("$1,310.00"), true, chain).as_deref(),
      Some("Portfolio refreshed. Value $1,310.00"),
      "a number the user's own Refresh moved is said, and said as the answer to that press"
   );

   assert_eq!(
      announcement(Some("$1,310.00"), Some("$1,310.00"), true, chain),
      None,
      "a refresh that found the same number has nothing to add"
   );

   // Leaving the page forgets what was read out, so coming back is an arrival again — with the
   // number as it is now, which may have moved while the page was away.
   assert_eq!(
      announcement(None, Some("$1,310.00"), false, chain).as_deref(),
      Some("Portfolio value on Ethereum, $1,310.00"),
      "coming back to the page announces the total once"
   );
}

/// Draw the portfolio page the way the frame does, out of the `SHARED_GUI` the frame gives it.
///
/// The rows are built on a worker that reads the handle from that global and writes them back into
/// it, so a `GUI` of the test's own would draw a header with nothing under it for ever.
fn render_portfolio(ctx: &Context) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, Vec::new(), |ui| {
      crate::gui::SHARED_GUI.write(|gui| {
         gui.portofolio.open();

         let zeus = gui.ctx.clone();
         let theme = gui.theme.clone();
         let icons = gui.icons.clone();
         let page = &mut gui.portofolio;
         let selection = &mut gui.token_selection;

         zeus.write(|zeus| page.show(zeus, &theme, icons, selection, ui));
      });
   })
}

/// The Home token page for the wallet with no vault, carrying this frame's input events.
///
/// The frame's one `SHARED_GUI` carries whatever the test before it left in the context, so the
/// wallet and the side the page draws from are pinned on every frame, and the rows the test walks
/// are the same ones on each pass.
fn render_home_tokens(ctx: &Context, events: Vec<Event>) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, events, |ui| {
      crate::gui::SHARED_GUI.write(|gui| {
         gui.portofolio.open();

         let zeus = gui.ctx.clone();
         let theme = gui.theme.clone();
         let icons = gui.icons.clone();

         zeus.write(|ctx| {
            let info = crate::core::WalletInfo::default();

            ctx.current_wallet = info.clone();
            ctx.privacy_mode = false;
            ctx.wallet_info_cache = crate::core::types::WalletInfoCache::new(
               std::collections::HashMap::from([(info.address, info.clone())]),
               vec![info],
            );
         });

         let page = &mut gui.portofolio;
         let selection = &mut gui.token_selection;

         zeus.write(|zeus| page.show(zeus, &theme, icons, selection, ui));
      });
   })
}

/// Every surface the portfolio paints says what it is: the asset cell is read by the token's name
/// where Zeus has one, the price, the balance and the value each say which figure they are, and the
/// total says what the number is and which chain it is on. The page also hands its arrival
/// sentence back to the frame, which owns the announcement channel.
#[test]
fn the_portfolio_names_its_total_and_every_cell() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();

   // The page reads its theme out of the context it draws into, and names the wallet it is on
   // screen for: that name comes from the cache unlock fills, so the zero address — the wallet a
   // context with no vault is on — is stood up in it here. No vault, RPC or icon is needed for the
   // rows themselves.
   crate::gui::SHARED_GUI.write(|gui| {
      let mut theme = gui.theme.clone();
      theme.install(&ctx);

      let zeus = gui.ctx.clone();

      zeus.write(|ctx| {
         let info = crate::core::WalletInfo::default();

         ctx.wallet_info_cache = crate::core::types::WalletInfoCache::new(
            std::collections::HashMap::from([(info.address, info.clone())]),
            vec![info],
         );
      });
   });

   let mut nodes = Vec::new();

   // The rows land a frame or two after the page opens, because they are built off the GUI thread.
   for _ in 0..200 {
      nodes = render_portfolio(&ctx);

      if nodes.iter().any(|(_, node)| node.label() == Some("Ethereum")) {
         break;
      }

      std::thread::sleep(std::time::Duration::from_millis(10));
   }

   let drawn: Vec<&str> = nodes.iter().filter_map(|(_, node)| node.label()).collect();

   let asset = nodes
      .iter()
      .find(|(_, node)| node.label() == Some("Ethereum"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| panic!("the asset cell must be read by the token's name: {drawn:?}"));

   assert_eq!(
      asset.description(),
      Some("ETH"),
      "the symbol the cell draws stays reachable as the description"
   );

   for sentence in ["Price $0.00", "Balance 0.00", "Value $0.00"] {
      assert!(
         nodes.iter().any(|(_, node)| node.label() == Some(sentence)),
         "the cell must say which figure it is, and {sentence:?} is missing from {drawn:?}"
      );
   }

   let total = "Portfolio value on Ethereum, $0.00";

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.value() == Some(total) || node.label() == Some(total)),
      "the total must say what the number is and which chain it is on: {drawn:?}"
   );

   assert_eq!(
      crate::gui::SHARED_GUI.write(|gui| gui.portofolio.take_notice()).as_deref(),
      Some(total),
      "the page must hand its arrival sentence to the frame, which says it"
   );
}

/// The Home token table is a keyboard path: every tab stop on the page says what it is, and the
/// walk leaves the row. The row's card was an interactive frame, which egui makes a focusable plate
/// with no name — the walk stopped on it and never left that row.
#[test]
fn every_tab_stop_on_the_home_token_table_has_a_name() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();

   // The rows land a frame or two after the page opens, because they are built off the GUI thread.
   let mut nodes = Vec::new();

   for _ in 0..200 {
      nodes = render_home_tokens(&ctx, Vec::new());

      if nodes.iter().any(|(_, node)| node.label() == Some("Ethereum")) {
         break;
      }

      std::thread::sleep(std::time::Duration::from_millis(10));
   }

   assert!(
      nodes.iter().any(|(_, node)| node.label() == Some("Ethereum")),
      "the token table must draw its first row: {:?}",
      drawn_names(&nodes)
   );

   let mut stops: Vec<String> = Vec::new();

   for _ in 0..40 {
      let nodes = render_home_tokens(&ctx, vec![key_press(Key::Tab)]);

      let Some(focused) = focused_id(&ctx) else {
         continue;
      };

      let spoken = nodes
         .iter()
         .find(|(node_id, _)| *node_id == focused)
         .map(|(_, node)| node.label().or_else(|| node.value()).unwrap_or_default())
         .unwrap_or_default()
         .to_string();

      assert!(
         !spoken.is_empty(),
         "a tab stop with nothing to say strands the keyboard: {:?}",
         drawn_names(&nodes)
      );

      stops.push(spoken);
   }

   assert!(
      stops.len() >= 3,
      "the page's controls must be on the keyboard path, and the walk stopped at {stops:?}"
   );
}

/// A reader can take a token row as one sentence: the asset, its price, the amount held and what
/// that amount is worth. The cells are labels, so without a control of its own the keyboard only
/// ever stopped on the row's Remove — the figures a reader came to the page for were unreachable.
///
/// The button is drawn for the reader model only: it is a glyph with a voice, and a tab stop that a
/// page drawn without a reader gains nothing from.
#[test]
fn a_reader_can_take_each_token_row_as_one_sentence() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();
   ctx.options_mut(|options| options.screen_reader = true);

   // The rows land a frame or two after the page opens, because they are built off the GUI thread.
   let mut nodes = Vec::new();

   for _ in 0..200 {
      nodes = render_home_tokens(&ctx, Vec::new());

      if nodes.iter().any(|(_, node)| node.label() == Some("Ethereum")) {
         break;
      }

      std::thread::sleep(std::time::Duration::from_millis(10));
   }

   let sentence = "Ethereum, price $0.00, balance 0.00, value $0.00";

   let speak = nodes
      .iter()
      .find(|(_, node)| node.label() == Some(sentence))
      .map(|(id, _)| *id)
      .unwrap_or_else(|| {
         panic!(
            "a row must be reachable as one sentence: {:?}",
            drawn_names(&nodes)
         )
      });

   // The page's arrival sentence is its own; this test reads what the row's own press says.
   crate::gui::SHARED_GUI.write(|gui| gui.portofolio.take_notice());

   render_home_tokens(&ctx, vec![accesskit_action(Action::Click, speak)]);

   assert_eq!(
      crate::gui::SHARED_GUI.write(|gui| gui.portofolio.take_notice()).as_deref(),
      Some(sentence),
      "pressing the row's button must say the row again, so a reader can hear it twice"
   );

   // No reader model, no button: the tab stop belongs to the reader, not to the page.
   ctx.options_mut(|options| options.screen_reader = false);
   let nodes = render_home_tokens(&ctx, Vec::new());

   assert!(
      !nodes.iter().any(|(_, node)| node.label() == Some(sentence)),
      "a page drawn without the reader model must not grow the button: {:?}",
      drawn_names(&nodes)
   );

   // Leave the shared page the way a fresh opening finds it: `close()` re-arms the arrival
   // sentence, and the next test draws this same page out of this same global.
   crate::gui::SHARED_GUI.write(|gui| gui.portofolio.close());
}
