use crate::core::ZeusCtx;
use crate::gui::SHARED_GUI;
use serde_json::Value;
use std::time::Duration;
use zeus_eth::{alloy_primitives::Address, alloy_signer::Signature, types::ChainId};

use super::msg::SignMsgType;

/// Releases the signing window's claim when the flow holding it ends.
///
/// A drop guard rather than a call at each exit point: `sign_message` must hand the window back on
/// every path, including a future dropped mid-prompt, so a flow that dies can never wedge the next
/// request.
struct SignClaim;

impl Drop for SignClaim {
   fn drop(&mut self) {
      SHARED_GUI.write(|gui| gui.sign_msg_window.release());
   }
}

/// Prompt the user to sign a message.
///
/// `signer` is the wallet that must produce the signature. Pass `None` to use
/// the currently selected wallet.
pub async fn sign_message(
   ctx: ZeusCtx,
   dapp: String,
   chain: ChainId,
   msg_value: Option<Value>,
   msg_bytes: Option<Vec<u8>>,
   signer: Option<Address>,
) -> Result<Signature, anyhow::Error> {
   ctx.ensure_unlocked()?;

   let signer_address = signer.unwrap_or_else(|| ctx.current_wallet_info().address);

   let msg_type = SignMsgType::new(ctx.clone(), chain.id(), msg_value, msg_bytes).await?;

   // One prompt at a time: two racing requests would share this window and both act on one answer.
   let busy = SHARED_GUI.write(|gui| {
      // Also guard against an already open tx window
      if gui.sign_msg_window.is_busy() || gui.tx_confirmation_window.is_busy() {
         return true;
      }

      gui.loading_window.reset();
      gui.sign_msg_window.open(dapp, chain.id(), msg_type.clone(), signer_address);
      gui.sign_msg_window.claim();
      gui.bring_to_front();
      false
   });

   if busy {
      return Err(anyhow::anyhow!(
         "Another signing request is already waiting for confirmation"
      ));
   }

   // Held until this flow is done with the prompt, however it ends: the claim is released on
   // return, on error, and if the future is dropped, so a flow that dies can never wedge the next
   // request.
   let _claim = SignClaim;

   // Wait for the user to sign or cancel
   let signed = loop {
      tokio::time::sleep(Duration::from_millis(50)).await;

      // Collect the answer and close in one exclusive step: the answer is single-use, so one Sign
      // click can only ever be acted on once.
      let answer = SHARED_GUI.write(|gui| {
         let signed = gui.sign_msg_window.take_signed();
         if signed.is_some() {
            gui.sign_msg_window.close();
         }
         signed
      });

      if let Some(signed) = answer {
         break signed;
      }
   };

   if !signed {
      SHARED_GUI.request_repaint();
      return Err(anyhow::anyhow!(
         "You cancelled the signing process"
      ));
   }

   // The wallet may have been locked while the prompt was up — never sign for a
   // locked wallet.
   ctx.ensure_unlocked()?;

   let wallet = ctx.get_wallet(signer_address).ok_or(anyhow::anyhow!("Wallet not found"))?;
   let signature = msg_type.sign(wallet.key).await?;

   SHARED_GUI.write(|gui| {
      gui.request_repaint();
   });

   Ok(signature)
}
