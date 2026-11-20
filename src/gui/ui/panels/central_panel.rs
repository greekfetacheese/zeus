use crate::core::ZeusContext;
use crate::gui::{GUI, ui::common::heading};
use eframe::egui::{RichText, Stroke, Ui, vec2};
use egui_elements::{Theme, widgets::Window};

pub fn show(gui: &mut GUI, ctx: &mut ZeusContext, ui: &mut Ui) {
   // Locked: only the lock card is shown, and nothing else is interactive.
   if ctx.locked {
      // Clear the page-focus marker so unlocking lands on the Home heading instead of
      // wherever focus happened to be before the lock.
      gui.last_page = None;

      // No pill is drawn while locked, so its message from before the lock is not the pill the
      // user comes back to: forgotten here, the next disappearance of a pill announces a
      // completion that really did just happen.
      gui.last_status_pill = None;

      // Cancel anything waiting on the user to authorize (a signature, a tx
      // confirmation, a connect prompt) before hiding it, so a prompt open when
      // the lock fired cannot be approved — or resurface after unlock.
      gui.cancel_pending_actions();

      // A private key / seed shown before the lock must not resurface after
      // unlock.
      gui.wallet_ui.lock(ui.ctx());

      // The delegate form lives in the account panel; lock it with the rest.
      gui.account_panel.lock();

      gui.lock_screen.show(ctx, &gui.theme, ui);

      // Informational modals (the unlock error, "saving vault…") must render on
      // the lock screen. Deliberately not `show_overlay_modals`: its action
      // prompts are cancelled above, so only the informational two are shown.
      gui.msg_window.show(&gui.theme, ui);
      gui.loading_window.show(&gui.theme, ui);

      // The loading window cannot announce from inside its own draw — the frame holds the only
      // channel — so its message comes back here and is said once over the lock screen.
      if let Some(text) = gui.loading_window.take_notice() {
         gui.announce(text);
      }
      return;
   }

   // The first widget of an unlocked frame names the page about to be drawn. A reader hears a
   // heading's name when focus lands on it, so a page change moves focus here instead of
   // announcing the page: the name is said once, and the Tab order starts at the page.
   let page = gui.current_page();
   let page_heading = heading(ui, page.name());

   if gui.last_page != Some(page) {
      page_heading.request_focus();
      gui.last_page = Some(page);
   }

   // Settings is a window over this panel, drawn by the same pass, so the dialogs are drawn once
   // here and are visible over either.
   gui.show_overlay_modals(ui);

   let theme = &gui.theme;
   let icons = gui.icons.clone();
   let token_selection = &mut gui.token_selection;
   let recipient_selection = &mut gui.recipient_selection;
   let contacts_ui = &mut gui.settings.contacts_ui;

   gui.recover_wallet_ui.show(ctx, theme, &mut gui.settings.import, ui);
   gui.unlock_vault_ui.show(ctx, theme, ui);
   gui.settings.import.show(theme, ui);

   // Vault must be unlocked so the wallet cache is populated
   if !ctx.vault_unlocked {
      return;
   }

   let chain_id = ctx.chain.id();
   let owner = ctx.current_wallet_info().address;

   token_selection.show(ctx, theme, icons.clone(), chain_id, owner, ui);

   gui.tx_confirmation_window.show(
      ctx,
      theme,
      icons.clone(),
      &gui.appearance_settings,
      ui,
   );

   // The confirmation draws from a borrowed theme as well, so its one sentence comes back here
   // like the Send form's. It covers every other surface while it is up, so it is said last.
   let confirmation_notice = gui.tx_confirmation_window.take_notice();

   gui.tx_window.show(ctx, theme, icons.clone(), ui);
   gui.spent_note_window.show(ctx, theme, icons.clone(), ui);

   gui.sign_msg_window.show(
      ctx,
      theme,
      icons.clone(),
      &gui.appearance_settings,
      ui,
   );

   // The signing prompt draws from a borrowed theme too, so its one sentence comes back here like
   // the confirmation's.
   let sign_notice = gui.sign_msg_window.take_notice();

   gui.across_bridge.show(
      ctx,
      theme,
      icons.clone(),
      recipient_selection,
      contacts_ui,
      ui,
   );

   // The bridge page draws from the central panel and has no channel of its own — the frame
   // holds the only one — so the sentence it raised comes back here, like the Send form's.
   let across_notice = gui.across_bridge.take_notice();

   gui.send_crypto.show(
      ctx,
      icons.clone(),
      theme,
      token_selection,
      recipient_selection,
      contacts_ui,
      ui,
   );

   // The Send form has no channel of its own — the frame holds the only one — so the sentence it
   // wants said comes back here, like the portfolio's.
   let send_notice = gui.send_crypto.take_notice();

   gui.portofolio.show(ctx, theme, icons.clone(), token_selection, ui);

   // The portfolio page has no channel of its own — the frame holds the only one — so it hands its
   // one sentence back here, and it is said below rather than from inside the page.
   let portfolio_notice = gui.portofolio.take_notice();

   gui.uniswap.show(ctx, theme, icons.clone(), token_selection, ui);

   // The Uniswap window cannot announce from inside its own draw — the frame holds the channel —
   // so the sentences its pages raise come back here.
   let uniswap_notice = gui.uniswap.take_notice();

   gui.shield_ui.show(
      ctx,
      theme,
      icons.clone(),
      token_selection,
      recipient_selection,
      contacts_ui,
      ui,
   );

   if let Some(currency) = gui.shield_ui.take_open_merge_notes() {
      gui.merge_notes_window.open(ctx, currency);
   }
   gui.merge_notes_window.show(ctx, theme, ui);

   // The picker is a modal over whichever page raised it and has no channel of its own: the frame
   // holds the only one, so its sentence comes back here like the Send form's and the portfolio's.
   let token_selection_notice = gui.token_selection.take_notice();

   gui.connected_dapps.show(ctx, theme, icons.clone(), ui);

   gui.wallet_ui.show(ctx, theme, icons.clone(), ui);

   // The Wallets page cannot announce from inside its own draw — the frame holds the channel — so
   // its one sentence comes back here.
   let wallet_notice = gui.wallet_ui.take_notice();

   gui.tx_history.show(ctx, theme, ui);

   // The history page draws from a borrowed theme too, so the count it says when it changes comes
   // back here, like the Send form's and the portfolio's.
   let tx_history_notice = gui.tx_history.take_notice();

   gui.approvals.show(ctx, theme, icons.clone(), ui);

   // The approvals page draws from a borrowed theme too, so its one sentence comes back here like
   // the portfolio's. A revoke finishes after the frame that started it.
   let approvals_notice = gui.approvals.take_notice();

   #[cfg(feature = "dev")]
   gui.dev.show(ctx, theme, icons, ui);

   #[cfg(feature = "dev")]
   gui.fps_metrics.show(theme, ui);

   #[cfg(feature = "dev")]
   {
      let theme = gui.editor.show(&mut gui.theme, ui);
      if let Some(theme) = theme {
         gui.theme = theme;
         gui.theme.install(ui.ctx());
      }
   }

   // Said last, once every surface has had its turn: the channel keeps the last call of a frame, so
   // these are ordered by how much of the screen they are about. They come after the developer
   // surfaces because announcing is the one thing here that needs the whole `GUI` mutably, and the
   // pages above draw from a borrowed theme.
   // The bridge page is drawn before the pages below it, so its sentence is said first.
   if let Some(text) = across_notice {
      gui.announce(text);
   }

   if let Some(text) = portfolio_notice {
      gui.announce(text);
   }

   if let Some(text) = send_notice {
      gui.announce(text);
   }

   // The wallets page draws before the history page, so its sentence is said before the history's.
   if let Some(text) = wallet_notice {
      gui.announce(text);
   }

   // A page's count is about the page under the prompts, so it is said before their answers.
   if let Some(text) = tx_history_notice {
      gui.announce(text);
   }

   // A revoke's answer is about the page it was pressed on, so the picker and the prompts drawn
   // over that page are said after it.
   if let Some(text) = approvals_notice {
      gui.announce(text);
   }

   // The Uniswap window is drawn over the page it was raised from, so its sentence is said before
   // the picker's, which covers the window.
   if let Some(text) = uniswap_notice {
      gui.announce(text);
   }

   // The picker covers the page that raised it, so its sentence is the one about what the user is
   // looking at, and the channel keeps the last call.
   if let Some(text) = token_selection_notice {
      gui.announce(text);
   }

   // The confirmation is the topmost surface there is: its answer is the last thing said.
   if let Some(text) = confirmation_notice {
      gui.announce(text);
   }

   // The signing prompt is drawn after the confirmation, so when both are up it is the one on
   // top: its answer is the last thing said.
   if let Some(text) = sign_notice {
      gui.announce(text);
   }
}

pub struct FPSMetrics {
   pub open: bool,
   pub max_fps: f64,
   pub time_ms: f64,
   pub time_micros: f64,
}

impl FPSMetrics {
   pub fn new() -> Self {
      Self {
         open: false,
         max_fps: 0.0,
         time_ms: 0.0,
         time_micros: 0.0,
      }
   }

   pub fn update(&mut self, time_ms: f64) {
      if time_ms > 0.0 {
         self.max_fps = 1_000.0 / time_ms;
         self.time_ms = time_ms;
         self.time_micros = time_ms * 1_000.0;
      } else {
         self.max_fps = 0.0;
         self.time_ms = 0.0;
         self.time_micros = 0.0;
      }
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      let mut open = self.open;

      let title = RichText::new("FPS Metrics").size(18.0);
      let window_frame = theme.window_frame;
      let title_frame = window_frame.stroke(Stroke::NONE);

      Window::new(title)
         .open(&mut open)
         .resizable(true)
         .collapsible(true)
         .movable(true)
         .title_frame(title_frame)
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_max_width(170.0);
            ui.set_max_height(130.0);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.xs);

               let max_fps = RichText::new(format!("Max FPS: {:.2}", self.max_fps)).size(14.0);
               ui.label(max_fps);

               let time_ms = RichText::new(format!("Time: {:.4} ms", self.time_ms)).size(14.0);
               ui.label(time_ms);

               let time_micros =
                  RichText::new(format!("Time: {:.2} μs", self.time_micros)).size(14.0);
               ui.label(time_micros);
            });
         });

      self.open = open;
   }
}
