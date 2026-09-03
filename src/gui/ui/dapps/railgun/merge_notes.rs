//! Merge Notes window consolidate fragmented private UTXOs via self-transfer.

use egui::{Order, RichText, ScrollArea, Spinner, Ui, vec2};
use egui_elements::{Button, Modal, Theme};

use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token},
   types::ChainId,
   utils::NumericValue,
};
use zeus_railgun::{
   RailgunSigner,
   caip::AssetId,
   poi::types::PoiStatus,
   transact::{MergeCandidate, MergeSuggestion, suggest_merge},
};

use crate::{
   core::ZeusContext,
   gui::{SHARED_GUI, ui::dapps::railgun::transfer::private_merge_notes},
   utils::RT,
};

const EXPLAIN: &str = "\
Railgun stores each shield as a separate private note (like UTXOs). \
Many small notes make unshields slower and can hit the circuit size limit \
(Zeus supports up to 5 notes per transaction).\n\n\
Merge Notes spends several small notes in one private transfer to yourself \
and creates a single larger note. Nothing leaves from your private balance, this is not an unshield.";

#[derive(Clone)]
enum MergeState {
   Idle,
   Loading,
   Ready(Option<MergeSuggestion>),
   Error(String),
   Sending,
}

pub struct MergeNotesWindow {
   open: bool,
   currency: Currency,
   chain_id: u64,
   owner: Address,
   state: MergeState,
   /// Cache key so stale async results are dropped.
   load_key: u64,
   size: (f32, f32),
}

impl MergeNotesWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         currency: Currency::from(ERC20Token::wrapped_native_token(1)),
         chain_id: 1,
         owner: Address::ZERO,
         state: MergeState::Idle,
         load_key: 0,
         size: (440.0, 450.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, ctx: &mut ZeusContext, currency: Currency) {
      self.open = true;
      self.currency = currency;
      self.chain_id = ctx.chain.id();
      self.owner = ctx.current_wallet_info().address;
      self.state = MergeState::Loading;
      self.load_key = self.load_key.wrapping_add(1);
      let key = self.load_key;
      let chain_id = self.chain_id;
      let asset = if self.currency.is_erc20() {
         AssetId::Erc20(self.currency.to_erc20().address)
      } else {
         self.state = MergeState::Error(
            "Merge requires an ERC-20 private balance (use WETH for ETH).".into(),
         );
         return;
      };

      RT.spawn(async move {
         let result = load_suggestion(chain_id, asset).await;
         SHARED_GUI.write(|gui| {
            let win = &mut gui.merge_notes_window;
            if win.load_key != key {
               return;
            }
            match result {
               Ok(suggestion) => win.state = MergeState::Ready(suggestion),
               Err(e) => win.state = MergeState::Error(e.to_string()),
            }
            gui.request_repaint();
         });
      });
   }

   pub fn close(&mut self) {
      self.open = false;
      self.state = MergeState::Idle;
   }

   pub fn reset(&mut self) {
      self.close();
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let mut open = self.open;

      // ? It doesnt actually dismiss despite closable set to tue, maybe i should switch to a Window

      Modal::new("Merge Private Notes", &mut open)
      .backdrop_order(Order::Background)
      .content_order(Order::Middle)
      .closable(true)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_max_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing = vec2(0.0, 12.0);
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               ui.label(
                  RichText::new(EXPLAIN)
                     .size(theme.typography.small)
                     .color(theme.colors.text),
               );

               ui.label(
                  RichText::new(format!("Token: {}", self.currency.symbol()))
                     .size(theme.typography.normal)
                     .strong(),
               );

               match &self.state {
                  MergeState::Idle | MergeState::Loading => {
                     ui.add(Spinner::new().size(28.0));
                     ui.label(
                        RichText::new("Scanning private notes…")
                           .size(theme.typography.normal),
                     );
                  }
                  MergeState::Error(msg) => {
                     ui.label(
                        RichText::new(msg)
                           .size(theme.typography.normal)
                           .color(theme.colors.error),
                     );
                     self.close_button(theme, ui);
                  }
                  MergeState::Ready(None) => {
                     ui.label(
                        RichText::new(
                           "No merge needed you already have at most one note per tree for this token.",
                        )
                        .size(theme.typography.normal)
                        .color(theme.colors.success),
                     );
                     self.close_button(theme, ui);
                  }
                  MergeState::Ready(Some(_)) => {
                     // Clone out of state so Confirm can mutably update it.
                     let suggestion = match &self.state {
                        MergeState::Ready(Some(s)) => s.clone(),
                        _ => unreachable!(),
                     };
                     self.show_suggestion(ctx, theme, &suggestion, ui);
                  }
                  MergeState::Sending => {
                  }
               }
            });
         });
   }

   fn show_suggestion(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      suggestion: &MergeSuggestion,
      ui: &mut Ui,
   ) {
      let decimals = self.currency.decimals();
      let amount = NumericValue::format_wei(U256::from(suggestion.amount), decimals);

      ui.label(RichText::new("Recommended merge").size(theme.typography.large).strong());

      ui.label(
         RichText::new(format!(
            "Merge {} notes -> 1 note  (circuit {})",
            suggestion.input_count(),
            suggestion.circuit_label()
         ))
         .size(theme.typography.normal),
      );

      ui.label(
         RichText::new(format!(
            "Amount: {} {}",
            amount.formatted(),
            self.currency.symbol()
         ))
         .size(theme.typography.normal),
      );

      // Clone suggestion for the confirm path (state may move on click).
      let amount_for_merge = amount.clone();
      let suggestion_for_display = suggestion.clone();

      ui.label(
         RichText::new(format!(
            "Notes for this token: {} -> {}{}",
            suggestion_for_display.notes_total_before,
            suggestion_for_display.notes_total_after,
            if suggestion_for_display.more_merges_available {
               "  (another merge may help after this)"
            } else {
               ""
            }
         ))
         .size(theme.typography.small)
         .color(theme.colors.text),
      );

      ui.label(
         RichText::new(format!(
            "UTXO tree #{}",
            suggestion_for_display.tree_number
         ))
         .size(theme.typography.small)
         .color(theme.colors.text),
      );

      ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
         ui.vertical_centered(|ui| {
            for (i, n) in suggestion_for_display.notes.iter().enumerate() {
               let v = NumericValue::format_wei(U256::from(n.amount), decimals);
               ui.label(
                  RichText::new(format!(
                     "  {}. {} {}  (leaf {})",
                     i + 1,
                     v.formatted(),
                     self.currency.symbol(),
                     n.leaf_index
                  ))
                  .size(theme.typography.small),
               );
            }
         });
      });

      ui.add_space(10.0);

      let button_size = vec2(ui.available_width() * 0.5, 45.0);
      let ui_size = vec2(ui.available_width(), 45.0);
      let visuals = theme.button_visuals();

      let confirm = Button::new(RichText::new("Confirm Merge").size(theme.typography.large))
         .visuals(visuals)
         .min_size(button_size);

      let cancel = Button::new(RichText::new("Cancel").size(theme.typography.large))
         .visuals(visuals)
         .min_size(button_size);

      ui.allocate_ui(ui_size, |ui| {
         ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;

            if ui.add(confirm).clicked() {
               self.start_merge(ctx, amount_for_merge);
            }

            if ui.add(cancel).clicked() {
               self.close();
            }
         });
      });
   }

   fn close_button(&mut self, theme: &Theme, ui: &mut Ui) {
      let visuals = theme.button_visuals();
      let button =
         Button::new(RichText::new("Close").size(theme.typography.normal)).visuals(visuals);
      if ui.add(button).clicked() {
         self.close();
      }
   }

   fn start_merge(&mut self, ctx: &mut ZeusContext, amount: NumericValue) {
      self.state = MergeState::Sending;
      self.close();
      let chain = ctx.chain;
      let currency = self.currency.clone();
      let from = self.owner;

      ctx.railgun_status.set_op_in_progress(chain.id(), true);

      RT.spawn_blocking(move || {
         let ctx_handle = SHARED_GUI.read(|gui| gui.ctx.clone());
         let result = RT.block_on(private_merge_notes(
            ctx_handle.clone(),
            chain,
            currency,
            amount,
            from,
         ));

         SHARED_GUI.write(|gui| {
            match result {
               Ok(_) => {
                  gui.merge_notes_window.close();
                  gui.loading_window.reset();
               }
               Err(e) => {
                  gui.merge_notes_window.state = MergeState::Error(e.to_string());
                  gui.loading_window.reset();
                  gui.notification.reset();
                  gui.msg_window.open(format!("Merge Error: {}", e.to_string()));
               }
            }
            gui.request_repaint();
         });

         ctx_handle.write(|c| {
            c.railgun_status.set_op_in_progress(chain.id(), false);
         });
      });
   }
}

async fn load_suggestion(
   chain_id: u64,
   asset: AssetId,
) -> Result<Option<MergeSuggestion>, anyhow::Error> {
   let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
   let chain = ChainId::new(chain_id).map_err(|e| anyhow::anyhow!("{e}"))?;
   if !ctx.railgun_is_supported(chain) {
      return Err(anyhow::anyhow!(
         "Railgun is not supported on this network"
      ));
   }

   if !ctx.is_railgun_enabled(chain.id()) {
      return Err(anyhow::anyhow!(
         "Railgun is disabled. Enable it in Settings/Railgun."
      ));
   }

   let wallet = ctx.get_current_wallet();
   if !wallet.can_derive_zk_address() {
      return Err(anyhow::anyhow!(
         "Current wallet cannot derive a Railgun address"
      ));
   }
   let seed = wallet.seed()?;
   let signer = RailgunSigner::from_seed(&seed, 0, chain_id)?;
   let address = signer.address().clone();

   ctx.sync_railgun(chain_id, false).await?;

   let mut provider = ctx.get_railgun_provider(chain_id, false).await?;
   let max_inputs = provider.max_merge_inputs();
   let notes = provider.notes(address).await;

   let candidates: Vec<MergeCandidate> = notes
      .into_iter()
      .filter(|n| {
         // Skip notes that POI marks unspendable when POI is on.
         match n.poi_status {
            Some(PoiStatus::Valid) | None => true,
            Some(_) => false,
         }
      })
      .map(|n| MergeCandidate {
         asset: n.asset,
         amount: n.amount,
         tree_number: n.tree_number,
         leaf_index: n.leaf_index,
      })
      .collect();

   Ok(suggest_merge(asset, &candidates, max_inputs))
}
