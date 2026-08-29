use crate::core::ZeusCtx;
use crate::gui::SHARED_GUI;
use serde_json::Value;
use std::time::Duration;
use zeus_eth::{alloy_primitives::Address, alloy_signer::Signature, types::ChainId};

use super::msg::SignMsgType;

/// Prompt the user to sign a message.
///
/// `signer` is the wallet that must produce the signature. Pass `None` to use
/// the currently selected wallet.
pub async fn sign_message(
   ctx: ZeusCtx,
   dapp: String,
   chain: ChainId,
   msg_value: Option<Value>,
   msg_string: Option<String>,
   signer: Option<Address>,
) -> Result<Signature, anyhow::Error> {
   let msg_type = SignMsgType::new(ctx.clone(), chain.id(), msg_value, msg_string).await?;

   SHARED_GUI.write(|gui| {
      gui.loading_window.reset();
      ctx.write(|ctx| {
         gui.sign_msg_window.open(ctx, dapp, chain.id(), msg_type.clone());
      });
      gui.request_repaint();
   });

   // Wait for the user to sign or cancel
   let mut signed = None;
   loop {
      tokio::time::sleep(Duration::from_millis(50)).await;

      SHARED_GUI.read(|gui| {
         signed = gui.sign_msg_window.is_signed();
      });

      if signed.is_some() {
         break;
      }
   }

   let signed = signed.unwrap();

   if !signed {
      SHARED_GUI.request_repaint();
      return Err(anyhow::anyhow!(
         "You cancelled the signing process"
      ));
   }

   let wallet = if let Some(address) = signer {
      ctx.get_wallet(address).ok_or(anyhow::anyhow!("Wallet not found"))?
   } else {
      ctx.get_current_wallet()
   };
   let signature = msg_type.sign(&wallet.key).await?;

   SHARED_GUI.write(|gui| {
      gui.request_repaint();
   });

   Ok(signature)
}
