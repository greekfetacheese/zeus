//! Dev-only: mint Zeus's Sepolia NFT test contracts from the DevUI.
//!
//! The contracts live in the `zeus-contracts` repo (`NftTest721.sol`, `NftTest1155.sol`) and pick
//! their art by token id, so one button per branch covers a different path of Zeus's image
//! pipeline: an on-chain `data:` URI with inline SVG, an `ipfs://` image that goes through the
//! gateway fallback list, a plain HTTPS image, and metadata with no image at all (the placeholder).
//! See that repo's `NFT_TESTNET.md` for the id ranges and the deployed addresses.
//!
//! Everything here mints **on Sepolia, to the active wallet**, and goes through the normal
//! `send_transaction` pipeline — so a mint shows the real confirm window and needs Sepolia ETH for
//! gas in the wallet you are testing with, rather than being a quiet background write.

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

/// `NftTest721` on Sepolia — ERC-721 + Metadata + Enumerable.
const NFT_TEST_721: Address = address!("0xaf5aa7b670ef209e23d3f7b39a8f42f84bd002ac");

/// `NftTest1155` on Sepolia — ERC-1155 + Metadata URI.
const NFT_TEST_1155: Address = address!("0x3E6F909dDBD068c6299ee2A47AD9FE44760D61E0");

/// The mint entry points of the test contracts. Not part of the app's ABI set: these are dev-only
/// test contracts, so their interface stays next to the dev-only code that calls it.
mod abi {
   use zeus_eth::alloy_sol_types::sol;

   sol! {
      /// Dev-only Sepolia test contract; see `zeus-contracts/NFT_TESTNET.md`.
      interface NftTest721 {
         function mint(address to) external returns (uint256);
         function mintTo(address to, uint256 tokenId) external returns (uint256);
      }

      /// Dev-only Sepolia test contract; see `zeus-contracts/NFT_TESTNET.md`.
      interface NftTest1155 {
         function mint(address to, uint256 id, uint256 value) external;
         function mintBatch(address to, uint256[] ids, uint256[] values) external;
      }
   }
}

/// Which art branch of `NftTest721` to mint.
#[derive(Clone, Copy)]
enum Branch721 {
   Svg,
   Ipfs,
   NoArt,
}

impl Branch721 {
   fn label(self) -> &'static str {
      match self {
         Self::Svg => "ERC-721 SVG art",
         Self::Ipfs => "ERC-721 IPFS art",
         Self::NoArt => "ERC-721 without art",
      }
   }
}

/// Which branch of `NftTest1155` to mint.
#[derive(Clone, Copy)]
enum Branch1155 {
   Svg,
   Ipfs,
   Https,
   NoArt,
   Batch,
}

impl Branch1155 {
   fn label(self) -> &'static str {
      match self {
         Self::Svg => "ERC-1155 SVG art",
         Self::Ipfs => "ERC-1155 IPFS art",
         Self::Https => "ERC-1155 HTTPS art",
         Self::NoArt => "ERC-1155 without art",
         Self::Batch => "ERC-1155 batch",
      }
   }
}

pub struct NftMinting {
   open: bool,
   size: (f32, f32),
}

impl NftMinting {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (520.0, 430.0),
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
               "Mints on Sepolia to the active wallet, through the normal confirm window. Each \
                button covers one branch of the image pipeline.",
            )
            .size(theme.typography.small),
         );

         ScrollArea::vertical().show(ui, |ui| {
            ui.label(RichText::new(format!("ERC-721 · {NFT_TEST_721}")).size(text_size));

            if mint_button(
               ui,
               theme,
               button_size,
               "Mint SVG art",
               "On-chain data: URI with inline SVG — no network needed",
            ) {
               spawn_mint_721(Branch721::Svg);
            }

            if mint_button(
               ui,
               theme,
               button_size,
               "Mint IPFS art",
               "ipfs:// image, resolved through the gateway fallback list",
            ) {
               spawn_mint_721(Branch721::Ipfs);
            }

            if mint_button(
               ui,
               theme,
               button_size,
               "Mint without art",
               "Metadata with no image, so Zeus falls back to the placeholder",
            ) {
               spawn_mint_721(Branch721::NoArt);
            }

            ui.add_space(theme.spacing.md);
            ui.label(RichText::new(format!("ERC-1155 · {NFT_TEST_1155}")).size(text_size));

            if mint_button(
               ui,
               theme,
               button_size,
               "Mint SVG art (id 0)",
               "On-chain data: URI with inline SVG — no network needed",
            ) {
               spawn_mint_1155(Branch1155::Svg);
            }

            if mint_button(
               ui,
               theme,
               button_size,
               "Mint IPFS art (id 1)",
               "ipfs:// image, resolved through the gateway fallback list",
            ) {
               spawn_mint_1155(Branch1155::Ipfs);
            }

            if mint_button(
               ui,
               theme,
               button_size,
               "Mint HTTPS art (id 2)",
               "Plain HTTPS image, no gateway involved",
            ) {
               spawn_mint_1155(Branch1155::Https);
            }

            if mint_button(
               ui,
               theme,
               button_size,
               "Mint without art (id 1000)",
               "Metadata with no image, so Zeus falls back to the placeholder",
            ) {
               spawn_mint_1155(Branch1155::NoArt);
            }

            if mint_button(
               ui,
               theme,
               button_size,
               "Mint batch (ids 3, 4)",
               "Two ids in one mintBatch call, with amounts 7 and 9",
            ) {
               spawn_mint_1155(Branch1155::Batch);
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

/// Send one `NftTest721` mint for `branch`, reporting the outcome in a toast.
fn spawn_mint_721(branch: Branch721) {
   RT.spawn(async move {
      let ctx = SHARED_GUI.write(|gui| gui.ctx.clone());
      let to = ctx.current_wallet_info().address;

      let call_data: Bytes = match branch {
         // `mint(to)` walks the contract's own counter, so this branch cannot collide with an id
         // that already exists.
         Branch721::Svg => abi::NftTest721::mintCall { to }.abi_encode().into(),
         // The other branches need an id inside their range, so take one from the clock: always
         // in range, and different enough that repeat clicks do not hit an id that exists.
         Branch721::Ipfs => {
            let token_id = U256::from(1000 + secs() % 1000);
            abi::NftTest721::mintToCall {
               to,
               tokenId: token_id,
            }
            .abi_encode()
            .into()
         }
         Branch721::NoArt => {
            let token_id = U256::from(2000 + secs() % 1000);
            abi::NftTest721::mintToCall {
               to,
               tokenId: token_id,
            }
            .abi_encode()
            .into()
         }
      };

      let result = mint(ctx, NFT_TEST_721, call_data).await;
      mint_toast(result, branch.label());
   });
}

/// Send one `NftTest1155` mint for `branch`, reporting the outcome in a toast.
fn spawn_mint_1155(branch: Branch1155) {
   RT.spawn(async move {
      let ctx = SHARED_GUI.write(|gui| gui.ctx.clone());
      let to = ctx.current_wallet_info().address;

      // Fixed ids here, unlike the 721: an ERC-1155 balance accumulates, so re-minting an id that
      // already exists is fine and repeat clicks just top the balance up.
      let call_data: Bytes = match branch {
         Branch1155::Svg => {
            let id = U256::from(0);
            abi::NftTest1155::mintCall {
               to,
               id,
               value: U256::ONE,
            }
            .abi_encode()
            .into()
         }
         Branch1155::Ipfs => {
            let id = U256::from(1);
            abi::NftTest1155::mintCall {
               to,
               id,
               value: U256::from(3),
            }
            .abi_encode()
            .into()
         }
         Branch1155::Https => {
            let id = U256::from(2);
            abi::NftTest1155::mintCall {
               to,
               id,
               value: U256::ONE,
            }
            .abi_encode()
            .into()
         }
         Branch1155::NoArt => {
            let id = U256::from(1000);
            abi::NftTest1155::mintCall {
               to,
               id,
               value: U256::from(5),
            }
            .abi_encode()
            .into()
         }
         Branch1155::Batch => abi::NftTest1155::mintBatchCall {
            to,
            ids: vec![U256::from(3), U256::from(4)],
            values: vec![U256::from(7), U256::from(9)],
         }
         .abi_encode()
         .into(),
      };

      let result = mint(ctx, NFT_TEST_1155, call_data).await;
      mint_toast(result, branch.label());
   });
}

/// Send one mint on Sepolia, through the normal pipeline (simulate, confirm, broadcast).
async fn mint(ctx: ZeusCtx, contract: Address, call_data: Bytes) -> Result<(), anyhow::Error> {
   let owner = ctx.current_wallet_info().address;

   let mut req = SendTxRequest::new(ChainId::EthereumSepolia, owner, contract)
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
            Toast::new(format!("Minted — {label}"))
               .description("Sepolia, to the active wallet")
               .tone(BadgeTone::Ok)
               .show(&gui.egui_ctx);
         }
         Err(e) => {
            Toast::new("Mint failed")
               .description(format!("{label}: {e}"))
               .tone(BadgeTone::Danger)
               .show(&gui.egui_ctx);
         }
      };

      gui.loading_window.reset();
      gui.request_repaint();
   });
}

/// Seconds since the epoch, used to pick an id inside a branch's range.
fn secs() -> u64 {
   std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|d| d.as_secs())
      .unwrap_or_default()
}
