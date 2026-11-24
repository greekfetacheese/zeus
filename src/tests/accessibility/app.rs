use super::harness::{accesskit_action, frame, frame_nodes};
use crate::assets::icons::Icons;
use crate::core::{WalletInfo, ZeusCtx};
use crate::gui::appearance::AppearanceSettings;
use crate::gui::{GUI, LockScreen};
use egui::accesskit::{Action, Node, NodeId, Role};
use egui::{Context, Event};
use egui_elements::{Theme, ThemeKind};
use std::sync::Arc;

/// The unlock screen and what it needs: the simplest real screen Zeus has — a heading, a line
/// of prose, the credentials form and its Unlock button — and the only one that renders without
/// a wallet, an RPC or a single icon.
pub(super) fn unlock_screen() -> (ZeusCtx, LockScreen, Theme) {
   let locked = ZeusCtx::new(crate::runtime::test_handle());
   locked.write(|ctx| ctx.locked = true);

   (
      locked,
      LockScreen::new(),
      Theme::new(ThemeKind::TokyoNight),
   )
}

/// Render the unlock screen and return the tree of its second frame.
///
/// Two passes on purpose: the screen is an `egui::Window`, which is measured on its first one,
/// and egui exposes no node for a widget it considers invisible (`is_accesskit_visible`).
pub(super) fn render_unlock_screen(
   ctx: &Context,
   locked: &ZeusCtx,
   screen: &mut LockScreen,
   theme: &Theme,
) -> Vec<Node> {
   frame(ctx, |ui| {
      locked.write(|zeus| screen.show(zeus, theme, ui));
   });

   frame(ctx, |ui| {
      locked.write(|zeus| screen.show(zeus, theme, ui));
   })
}

/// The main window with no wallet open behind it: the nav and the page heading draw, while the
/// pages they navigate to stay closed. Nothing here needs a vault, an RPC or an icon.
pub(super) fn main_window() -> (Context, GUI, ZeusCtx) {
   let ctx = Context::default();
   let mut gui = GUI::default();
   gui.theme.install(&ctx);

   (
      ctx,
      gui,
      ZeusCtx::new(crate::runtime::test_handle()),
   )
}

/// Render the left panel headlessly, carrying this frame's input events.
pub(super) fn render_nav(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, events, |ui| {
      zeus.write(|zeus| crate::gui::ui::panels::left_panel::show(gui, zeus, ui));
   })
}

/// Draw the left panel and the announcer, which is the order the app draws them in: the panel
/// hands the account header's copy to the app's one channel, and the announcer says it once.
pub(super) fn render_left_panel(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, events, |ui| {
      zeus.write(|zeus| {
         crate::gui::ui::panels::left_panel::show(gui, zeus, ui);
         gui.show_announcer(ui);
      });
   })
}

/// Render the central panel headlessly: the page heading is its first widget.
pub(super) fn render_central(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, events, |ui| {
      zeus.write(|zeus| crate::gui::ui::panels::central_panel::show(gui, zeus, ui));
   })
}

/// The main window with Settings open over it: opening the window needs no vault, RPC or icon.
pub(super) fn settings_window() -> (Context, GUI, ZeusCtx) {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| gui.settings.open(zeus, &mut gui.appearance_settings));

   (ctx, gui, zeus)
}

/// Render the Settings window headlessly, twice: an `egui::Window` is measured on its first pass,
/// and a page is drawn through the fade helper.
///
/// `events` go to the first pass only, so a key that closes the window is not pressed twice.
pub(super) fn render_settings(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for pass in 0..2 {
      let icons = gui.icons.clone();
      let theme = gui.theme.clone();

      let events = if pass == 0 {
         events.clone()
      } else {
         Vec::new()
      };

      nodes = frame_nodes(ctx, events, |ui| {
         zeus.write(|zeus| {
            gui.settings.show(
               zeus,
               icons.clone(),
               &theme,
               &mut gui.appearance_settings,
               &mut gui.keymap,
               ui,
            );
         });
      });
   }

   nodes
}

/// Render the Settings window and the announcer, which is the order the app draws them in: a page
/// raises its words on the window, and the window hands them to the app's one channel. `events`
/// go to the last pass, so the frame returned is the one that answers them.
pub(super) fn render_settings_speaking(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for pass in 0..2 {
      let icons = gui.icons.clone();
      let theme = gui.theme.clone();
      let events = if pass == 1 {
         events.clone()
      } else {
         Vec::new()
      };

      nodes = frame_nodes(ctx, events, |ui| {
         zeus.write(|zeus| {
            gui.settings.show(
               zeus,
               icons.clone(),
               &theme,
               &mut gui.appearance_settings,
               &mut gui.keymap,
               ui,
            );

            if let Some(text) = gui.settings.take_notice() {
               gui.announce(text);
            }

            gui.show_announcer(ui);
         });
      });
   }

   nodes
}

/// Render the central panel and then the Settings window in the order the app draws them: the panel
/// raises the overlay dialogs, and Settings is a window over it.
pub(super) fn render_central_with_settings(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, events, |ui| {
      zeus.write(|zeus| {
         crate::gui::ui::panels::central_panel::show(gui, zeus, ui);

         let icons = gui.icons.clone();
         let theme = gui.theme.clone();

         gui.settings.show(
            zeus,
            icons,
            &theme,
            &mut gui.appearance_settings,
            &mut gui.keymap,
            ui,
         );
      });
   })
}

/// Open a Settings page the way a reader does — an AccessKit Click on its tab — and return the
/// tree of the page that opened.
pub(super) fn open_settings_page(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   page: &str,
) -> Vec<(NodeId, Node)> {
   let nodes = render_settings(ctx, gui, zeus, Vec::new());
   let tab = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Tab && node.label() == Some(page))
      .map(|(id, _)| *id)
      .unwrap_or_else(|| panic!("the {page} page must be a tab in the tree"));

   render_settings(
      ctx,
      gui,
      zeus,
      vec![accesskit_action(Action::Click, tab)],
   )
}

/// Stand the zero-address wallet up in the cache the Send form reads its wallet from.
///
/// Without a vault that cache is empty, and the form asks it which wallet it is on screen for.
pub(super) fn wallet_in_cache(zeus: &ZeusCtx) {
   zeus.write(|ctx| {
      let info = crate::core::WalletInfo::default();

      ctx.wallet_info_cache = crate::core::types::WalletInfoCache::new(
         std::collections::HashMap::from([(info.address, info.clone())]),
         vec![info],
      );
   });
}

/// The turn every test that draws out of the frame's one `SHARED_GUI` takes.
///
/// That global holds the picker and the portfolio page together, and the page takes whatever the
/// picker holds as its selection — it resets the picker as soon as one arrives. Two tests drawing it
/// at once would take each other's rows and selections, so they take turns.
pub(super) static SHARED_GUI_TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(super) fn shared_gui_turn() -> std::sync::MutexGuard<'static, ()> {
   SHARED_GUI_TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The pieces a prompt window's frame gives it: the installed theme, the icon store its rows
/// draw with, and the appearance settings its hold is read from.
pub(super) fn prompt_harness() -> (
   Context,
   ZeusCtx,
   Theme,
   Arc<Icons>,
   AppearanceSettings,
) {
   let ctx = Context::default();
   let mut theme = Theme::new(ThemeKind::TokyoNight);
   theme.install(&ctx);
   egui_extras::install_image_loaders(&ctx);

   let icons = Arc::new(Icons::new(&ctx).expect("icons"));
   let handle = ZeusCtx::new(crate::runtime::test_handle());

   (
      ctx,
      handle,
      theme,
      icons,
      AppearanceSettings::new(),
   )
}

/// A wallet named `name` holding a random key: imported until a test says otherwise.
pub(super) fn wallet_named(name: &str) -> WalletInfo {
   WalletInfo::from_wallet(
      &zeus_wallet::Wallet::new_rng(name.to_string()),
      false,
   )
}

/// The master wallet a vault starts with.
pub(super) fn master_wallet(name: &str) -> WalletInfo {
   let mut wallet = wallet_named(name);
   wallet.is_master = true;
   wallet.is_imported = false;
   wallet
}
