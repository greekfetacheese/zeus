use crate::core::ZeusCtx;
use crate::gui::SHARED_GUI;
use serde_json::Value;
use std::time::Duration;
use zeus_eth::{alloy_signer::Signature, types::ChainId};

use super::msg::SignMsgType;

/// Prompt the user to sign a message
pub async fn sign_message(
   ctx: ZeusCtx,
   dapp: String,
   chain: ChainId,
   msg_value: Option<Value>,
   msg_string: Option<String>,
) -> Result<Signature, anyhow::Error> {
   let msg_type = SignMsgType::new(ctx.clone(), chain.id(), msg_value, msg_string).await?;

   SHARED_GUI.write(|gui| {
      gui.loading_window.reset();
      gui.sign_msg_window.open(ctx.clone(), dapp, chain.id(), msg_type.clone());
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
         SHARED_GUI.write(|gui| {
            gui.sign_msg_window.close(ctx.clone());
         });
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

   let secure_signer = ctx.get_current_wallet().key;
   let signature = msg_type.sign(&secure_signer).await?;

   SHARED_GUI.write(|gui| {
      gui.request_repaint();
   });

   Ok(signature)
}
