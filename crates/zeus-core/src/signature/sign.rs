use crate::ZeusCtx;
use crate::ui::{self, PromptOpen, SignRequest};
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
      ui::host().release_sign();
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

   // One prompt at a time, whatever raised it: every authorising prompt counts, not only this
   // window and the transaction confirmation it used to check by hand.
   match ui::host().open_sign(SignRequest {
      dapp,
      chain_id: chain.id(),
      msg_type: msg_type.clone(),
      signer: signer_address,
   }) {
      PromptOpen::Opened => {}
      PromptOpen::Busy => {
         return Err(anyhow::anyhow!(
            "Another signing request is already waiting for confirmation"
         ));
      }
      PromptOpen::Unavailable => return Err(anyhow::anyhow!(ui::NO_UI_MSG)),
   }

   // Held until this flow is done with the prompt, however it ends: the claim is released on
   // return, on error, and if the future is dropped, so a flow that dies can never wedge the next
   // request.
   let _claim = SignClaim;

   // Wait for the user to sign or cancel
   let signed = loop {
      tokio::time::sleep(Duration::from_millis(50)).await;

      if let Some(signed) = ui::host().take_sign() {
         break signed;
      }
   };

   if !signed {
      ui::host().request_repaint();
      return Err(anyhow::anyhow!(
         "You cancelled the signing process"
      ));
   }

   // The wallet may have been locked while the prompt was up — never sign for a
   // locked wallet.
   ctx.ensure_unlocked()?;

   let wallet = ctx.get_wallet(signer_address).ok_or(anyhow::anyhow!("Wallet not found"))?;
   let signature = msg_type.sign(wallet.key).await?;

   ui::host().request_repaint();

   Ok(signature)
}
