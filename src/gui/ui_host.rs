//! The GUI's implementation of [`crate::core::ui::UiHost`].
//!
//! Every method is one `SHARED_GUI` write around the same window calls the
//! pipelines used to make themselves. The composite shape is deliberate: the
//! busy-check, the `open` and the `claim` cannot be split without reopening the
//! race that lets a second prompt replace the one the user is reading.

use crate::core::tx::TransactionRich;
use crate::core::tx::notification::NotificationType;
use crate::core::ui::{
   ConfirmRequest, PromptOpen, SignRequest, TxConfirmAnswer, TxConfirmRequest, UiHost,
};
use crate::gui::SHARED_GUI;
use crate::gui::ui::common::windows::authorising_prompt_busy;

pub struct GuiHost;

impl UiHost for GuiHost {
   fn request_repaint(&self) {
      SHARED_GUI.request_repaint();
   }

   fn open_loading(&self, msg: &str) {
      SHARED_GUI.write(|gui| gui.loading_window.open(msg));
   }

   fn reset_loading(&self) {
      SHARED_GUI.write(|gui| gui.loading_window.reset());
   }

   fn open_tx_confirm(&self, req: TxConfirmRequest) -> PromptOpen {
      SHARED_GUI.write(|gui| {
         if authorising_prompt_busy(gui) {
            return PromptOpen::Busy;
         }

         gui.tx_confirmation_window.open(
            req.ctx,
            req.source_is_zeus,
            req.dapp,
            req.chain,
            req.analysis,
            req.priority_fee,
            req.mev_protect,
            req.sponsored,
            req.recipient_name,
         );

         // `open` refuses a window that is already busy, so the claim lands after it.
         // Both happen under this one lock, so the window reads busy to another flow
         // the moment this returns: a second send cannot slip in before the prompt is drawn.
         gui.tx_confirmation_window.claim();

         gui.loading_window.reset();
         gui.bring_to_front();
         PromptOpen::Opened
      })
   }

   fn take_tx_confirm(&self) -> Option<TxConfirmAnswer> {
      SHARED_GUI.write(|gui| gui.tx_confirmation_window.take_answer())
   }

   fn release_tx_confirm(&self) {
      SHARED_GUI.write(|gui| gui.tx_confirmation_window.release());
   }

   fn open_sign(&self, req: SignRequest) -> PromptOpen {
      SHARED_GUI.write(|gui| {
         if authorising_prompt_busy(gui) {
            return PromptOpen::Busy;
         }

         gui.loading_window.reset();
         gui.sign_msg_window.open(req.dapp, req.chain_id, req.msg_type, req.signer);
         gui.sign_msg_window.claim();
         gui.bring_to_front();
         PromptOpen::Opened
      })
   }

   fn take_sign(&self) -> Option<bool> {
      SHARED_GUI.write(|gui| gui.sign_msg_window.take_answer())
   }

   fn release_sign(&self) {
      SHARED_GUI.write(|gui| gui.sign_msg_window.release());
   }

   fn open_confirm(&self, req: ConfirmRequest) -> PromptOpen {
      SHARED_GUI.write(|gui| {
         if authorising_prompt_busy(gui) {
            return PromptOpen::Busy;
         }

         let opened = if req.from_dapp {
            gui.confirm_window.open_from_dapp(req.title)
         } else {
            gui.confirm_window.open(req.title)
         };

         if !opened {
            return PromptOpen::Busy;
         }

         if let Some(msg2) = req.msg2 {
            gui.confirm_window.set_msg2(msg2);
         }
         gui.request_repaint();
         PromptOpen::Opened
      })
   }

   fn take_confirm(&self) -> Option<bool> {
      SHARED_GUI.write(|gui| gui.confirm_window.take_answer())
   }

   fn release_confirm(&self) {
      SHARED_GUI.write(|gui| gui.confirm_window.release());
   }

   fn notify_pending(&self, title: &str, kind: NotificationType) {
      SHARED_GUI.write(|gui| {
         gui.notification.open_with_spinner(title.to_string(), kind);
         gui.request_repaint();
      });
   }

   fn notify_mined(
      &self,
      title: &str,
      kind: NotificationType,
      tx: TransactionRich,
      now_ms: u64,
      finish_ms: u64,
   ) {
      SHARED_GUI.write(|gui| {
         gui.notification.open_with_progress_bar(
            now_ms,
            finish_ms,
            title.to_string(),
            kind,
            Some(tx),
         );
         gui.request_repaint();
      });
   }

   fn lockout_and_shutdown(&self, msg: &str) {
      SHARED_GUI.write(|gui| {
         gui.open_msg_window(msg);
         gui.shutdown();
      });
   }
}
