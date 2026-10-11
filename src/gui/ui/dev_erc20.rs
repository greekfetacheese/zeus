//! Dev-only: mint Zeus's Sepolia test ERC-20 from the DevUI.
//!
//! The token lives in the `zeus-contracts` repo (`ZeusTestERC20.sol`); its `mint` is permissionless
//! and takes an amount in base units, so each button here is one fixed amount — one at the awkward
//! low end of Zeus's amount formatting, two at sizes worth eyeballing. See that repo's
//! `ERC20_TESTNET.md` for the address and the full surface.
//!
//! Everything here runs **on Sepolia, against the active wallet**, and goes through the normal
//! `send_transaction` pipeline — so it shows the real confirm window and needs Sepolia ETH for gas in
//! the wallet you are testing with, rather than being a quiet background write.

use eframe::egui::{RichText, ScrollArea, Ui, Vec2, vec2};
use egui_elements::{Button, Theme};
use elegance::{BadgeTone, Toast};
use zeus_eth::{
   alloy_primitives::{Address, Bytes, U256, address},
   alloy_sol_types::SolCall,
   types::ChainId,
};

use crate::core::{SendTxOptions, SendTxRequest, ZeusCtx, send_transaction};
use crate::gui::SHARED_GUI;
use crate::utils::RT;

/// `ZeusTestERC20` on Sepolia — ERC-20 + EIP-2612 permit.
const TEST_ERC20: Address = address!("0x263cDd6d30D057A60DBEDa0a1457ef9F236eFB42");

/// The token's own scale, as the contract reports it. Named here so the buttons can label their
/// amounts without a round trip.
const DECIMALS: u32 = 18;
const SYMBOL: &str = "ZEUS20";

/// The mint entry point of the test token. Not part of the app's ABI set: this is a dev-only test
/// contract, so its interface stays next to the dev-only code that calls it.
mod abi {
   use zeus_eth::alloy_sol_types::sol;

   sol! {
      /// Dev-only Sepolia test contract; see `zeus-contracts/ERC20_TESTNET.md`.
      interface ZeusTestERC20 {
         function mint(address to, uint256 value) external;
      }
   }
}

/// How much one mint button mints.
#[derive(Clone, Copy)]
enum MintAmount {
   /// A single base unit: the smallest amount Zeus has to render, so the awkward end of its amount
   /// formatting is reachable without hand-crafting a transaction.
   OneWei,
   Thousand,
   HundredThousand,
}

impl MintAmount {
   fn label(self) -> &'static str {
      match self {
         Self::OneWei => "1 wei",
         Self::Thousand => "1,000 ZEUS20",
         Self::HundredThousand => "100,000 ZEUS20",
      }
   }

   /// The amount in base units, which is what the contract takes.
   fn value(self) -> U256 {
      match self {
         Self::OneWei => U256::ONE,
         Self::Thousand => U256::from(1_000) * token_unit(),
         Self::HundredThousand => U256::from(100_000) * token_unit(),
      }
   }

   fn hint(self) -> &'static str {
      match self {
         Self::OneWei => "A single base unit — the smallest amount Zeus has to render",
         Self::Thousand => "A normal amount, easy to eyeball against the portfolio",
         Self::HundredThousand => "Enough to cover large transfers and swap-sized inputs",
      }
   }
}

/// One whole ZEUS20, i.e. the token's `decimals` applied.
fn token_unit() -> U256 {
   U256::from(10).pow(U256::from(DECIMALS))
}

pub struct Erc20Minting {
   open: bool,
   size: (f32, f32),
}

impl Erc20Minting {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (520.0, 240.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let size = self.size;

      ui.vertical_centered(|ui| {
         ui.set_width(size.0);
         ui.set_height(size.1);
         ui.spacing_mut().item_spacing.y = theme.spacing.md;

         let text_size = theme.typography.normal;
         let button_size = vec2(size.0, 40.0);

         ui.label(
            RichText::new(
               "Mints on Sepolia to the active wallet, through the normal confirm window. The \
                contract's mint is permissionless, so the balance is yours to spend.",
            )
            .size(theme.typography.small),
         );

         ScrollArea::vertical().show(ui, |ui| {
            ui.label(RichText::new(format!("{SYMBOL} · {TEST_ERC20}")).size(text_size));

            for amount in [
               MintAmount::OneWei,
               MintAmount::Thousand,
               MintAmount::HundredThousand,
            ] {
               let label = format!("Mint {}", amount.label());

               if mint_button(ui, theme, button_size, &label, amount.hint()) {
                  spawn_mint(amount);
               }
            }
         });
      });
   }
}

/// A mint button that carries its explanation as a tooltip.
fn mint_button(ui: &mut Ui, theme: &Theme, size: Vec2, label: &str, hint: &str) -> bool {
   ui.add(Button::new(RichText::new(label).size(theme.typography.normal)).min_size(size))
      .on_hover_text(hint)
      .clicked()
}

/// Send one `mint` of `amount` to the active wallet, reporting the outcome in a toast.
fn spawn_mint(amount: MintAmount) {
   RT.spawn(async move {
      let ctx = SHARED_GUI.write(|gui| gui.ctx.clone());
      let to = ctx.current_wallet_info().address;

      let call_data: Bytes = abi::ZeusTestERC20::mintCall {
         to,
         value: amount.value(),
      }
      .abi_encode()
      .into();

      let result = mint(ctx, call_data).await;
      mint_toast(result, amount.label());
   });
}

/// Send one mint on Sepolia, through the normal pipeline (simulate, confirm, broadcast).
async fn mint(ctx: ZeusCtx, call_data: Bytes) -> Result<(), anyhow::Error> {
   let owner = ctx.current_wallet_info().address;

   let mut req = SendTxRequest::new(ChainId::EthereumSepolia, owner, TEST_ERC20)
      .call_data(call_data)
      .value(U256::ZERO)
      .authorization_list(Vec::new());

   // No pre-built analysis: let the send simulate the call and build the confirm window.
   req.analysis = None;

   send_transaction(
      ctx,
      true,
      req,
      SendTxOptions {
         dapp: "Zeus Dev UI".to_string(),
         ..Default::default()
      },
   )
   .await?;

   Ok(())
}

/// Report a mint's outcome where the rest of the DevUI reports: a toast.
fn mint_toast(result: Result<(), anyhow::Error>, label: &str) {
   SHARED_GUI.write(|gui| {
      match result {
         Ok(()) => {
            Toast::new(format!("Minted {label}"))
               .description("Sepolia, to the active wallet")
               .tone(BadgeTone::Ok)
               .show(&gui.egui_ctx);
         }
         Err(e) => {
            Toast::new(format!("Mint failed — {label}"))
               .description(e.to_string())
               .tone(BadgeTone::Danger)
               .show(&gui.egui_ctx);
         }
      };

      gui.loading_window.reset();
      gui.request_repaint();
   });
}

#[cfg(test)]
mod tests {
   use super::*;

   /// The encoded call must be the one the deployed contract answers to — `mint(address,uint256)`,
   /// `0x40c10f19` — or every button in this window would revert. Pinned to the canonical selector
   /// rather than to the derived constant, so a change in the generated ABI fails here.
   #[test]
   fn mint_encodes_the_canonical_call() {
      let to = address!("0x8054f96990662150be89d559Ef249A15809567a9");
      let amount = U256::from(1_000);

      let call = abi::ZeusTestERC20::mintCall { to, value: amount }.abi_encode();

      assert_eq!(
         abi::ZeusTestERC20::mintCall::SELECTOR,
         [0x40, 0xc1, 0x0f, 0x19]
      );
      assert_eq!(
         &call[..4],
         &[0x40, 0xc1, 0x0f, 0x19],
         "mint(address,uint256) selector"
      );
      assert_eq!(
         call.len(),
         4 + 32 + 32,
         "address and uint256, both left-padded to 32 bytes"
      );
      assert_eq!(U256::from_be_slice(&call[36..]), amount);
   }

   /// The presets must land on the token's own scale. Whole tokens would be off by 10^18 and the
   /// contract would still accept the call, so nothing downstream would catch it.
   #[test]
   fn amounts_are_on_the_tokens_scale() {
      assert_eq!(MintAmount::OneWei.value(), U256::ONE);
      assert_eq!(
         MintAmount::Thousand.value(),
         U256::from(10u64).pow(U256::from(DECIMALS)) * U256::from(1_000)
      );
      assert_eq!(
         MintAmount::HundredThousand.value(),
         U256::from(10u64).pow(U256::from(DECIMALS)) * U256::from(100_000)
      );
   }
}
