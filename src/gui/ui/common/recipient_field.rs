//! The "Recipient" row shared by every send-like flow.
//!
//! Across, Send and Railgun shield/unshield all render the same thing: the
//! resolved name (or "Unknown Address"), a block-explorer link, and the address
//! field that opens [`RecipientSelectionWindow`]. One implementation means the
//! picker only has to be wired up once, and the three flows cannot drift apart.

use crate::assets::icons::Icons;
use crate::core::ZeusContext;
use crate::gui::ui::{ContactsUi, RecipientSelectionWindow};
use crate::utils::TimeStamp;
use eframe::egui::{
   CursorIcon, FontId, Key, Margin, OpenUrl, RichText, Role, Sense, Ui, WidgetInfo, vec2,
};
use egui_elements::{SecureTextEdit, Theme};
use egui_lucide::Lucide;
use std::sync::Arc;
use zeus_eth::types::ChainId;

/// Show the recipient picker and its field.
///
/// `privacy_mode` selects which side the picker lists and which address the
/// field edits (`0zk` in private mode, `0x` otherwise).
///
/// `send_chain` is the chain this flow will actually send the recipient to — the
/// active chain for send / unshield, the destination chain for a bridge. A
/// chain-specific name (`name@chain`) that disagrees with it is never sent
/// silently.
///
/// `explorer_chain` is the chain whose block explorer the link points at: the
/// active chain for send / unshield, the destination chain for a bridge.
///
/// The recipient is written back on [`RecipientSelectionWindow`], so callers read
/// it with [`RecipientSelectionWindow::get_recipient`] after this returns.
pub fn recipient_field(
   ctx: &mut ZeusContext,
   theme: &Theme,
   icons: Arc<Icons>,
   recipient_selection: &mut RecipientSelectionWindow,
   contacts_ui: &mut ContactsUi,
   privacy_mode: bool,
   send_chain: u64,
   explorer_chain: ChainId,
   ui: &mut Ui,
) {
   // The picker can change the recipient while it is up, so it is shown (and the
   // recipient re-read) before the row is drawn.
   recipient_selection.show(
      ctx,
      theme,
      icons,
      privacy_mode,
      send_chain,
      contacts_ui,
      ui,
   );

   let recipient = recipient_selection.get_recipient();

   // A name whose registration lapsed past its grace period may now belong to
   // someone else, so the address it resolved to is no longer what the name means.
   let now = TimeStamp::now_as_secs().unwrap_or_default().timestamp();
   let name_lapsed = !recipient.name_binding_trusted(now);

   theme.frame2.show(ui, |ui| {
      ui.set_width(ui.available_width());

      ui.horizontal(|ui| {
         let drawn = ui.label(RichText::new("Recipient").size(theme.typography.large));

         // The field carries this word as its name, so the label is not a second destination.
         ui.ctx().accesskit_node_builder(drawn.id, |node| node.set_hidden());

         ui.add_space(10.0);

         if !recipient.is_empty(privacy_mode) {
            if let Some(name) = &recipient.name {
               let name_color = match name_lapsed {
                  true => theme.colors.error,
                  false => theme.colors.info,
               };

               ui.label(RichText::new(name).size(theme.typography.large).color(name_color));

               if name_lapsed {
                  ui.label(
                     RichText::new(
                        "This name is past its registration and may no longer \
                         belong to the address it resolved to.",
                     )
                     .size(theme.typography.normal)
                     .color(theme.colors.error),
                  );
               }
            } else {
               ui.label(
                  RichText::new("Unknown Address")
                     .size(theme.typography.large)
                     .color(theme.colors.error),
               );
            }

            ui.add_space(5.0);

            if !privacy_mode && !recipient.evm_address.is_empty() {
               let link = format!(
                  "{}/address/{}",
                  explorer_chain.block_explorer(),
                  recipient.evm_address
               );

               let icon = Lucide::ExternalLink
                  .size(18.0)
                  .color(theme.colors.text)
                  .image()
                  .sense(Sense::click());

               let res = ui.add(icon).on_hover_cursor(CursorIcon::PointingHand);

               // The link is a picture with a click sense: no role and no words, so a reader has
               // nothing to land on. A clickable response is focusable and Enter or Space presses
               // it, so naming it is the whole of what it was missing.
               res.widget_info(|| {
                  WidgetInfo::labeled(
                     Role::Button,
                     ui.is_enabled(),
                     "Open the recipient in the block explorer",
                  )
               });

               if res.clicked() {
                  let url = OpenUrl::new_tab(link);
                  ui.ctx().open_url(url);
               }
            }
         }
      });

      ui.horizontal(|ui| {
         let hint = match privacy_mode {
            false => "Search contacts, ENS or enter an address",
            true => "Search contacts or enter a 0zk address",
         };

         let hint =
            RichText::new(hint).size(theme.typography.normal).color(theme.colors.text_muted);

         let address = if privacy_mode {
            &mut recipient_selection.recipient.zk_address
         } else {
            &mut recipient_selection.recipient.evm_address
         };

         let field_name = match privacy_mode {
            true => "Recipient 0zk address",
            false => "Recipient address",
         };

         let res = ui.add(
            SecureTextEdit::singleline(address)
               .accessible_name(field_name)
               .visuals(theme.text_edit_visuals())
               .hint_text(hint)
               .min_size(vec2(ui.available_width(), 25.0))
               .margin(Margin::same(10))
               .font(FontId::proportional(theme.typography.normal)),
         );

         // `changed()` is set only by the user's own input events in this frame (the widget marks
         // it from `secure_text_edit_events`, events it only reads while focused), never by the
         // programmatic write the picker performs when it accepts a name. So a `changed()` here
         // means the address is no longer the one the resolved name described, and the name,
         // its chain and its takeover deadline stop applying to it.
         if res.changed() {
            recipient_selection.recipient.forget_name_binding();
         }

         // The node says how much of an address the field holds and how it ends, never the address:
         // the whole thing is on screen, and the ending is what can be checked by ear against the
         // address meant to be pasted. The resolved name is the description, so the field can be
         // recognised without the address being read back.
         let typed = if privacy_mode {
            &recipient_selection.recipient.zk_address
         } else {
            &recipient_selection.recipient.evm_address
         };
         let held = recipient_value(typed);
         let resolved = recipient_selection.recipient.name.clone();
         let description = match &resolved {
            Some(name) => format!("{name}. Press Enter to choose a recipient."),
            None => "Press Enter to choose a recipient.".to_string(),
         };

         ui.ctx().accesskit_node_builder(res.id, |node| {
            if let Some(value) = &held {
               node.set_value(value.as_str());
            } else {
               node.clear_value();
            }

            node.set_description(description.as_str());
         });

         // The picker was reachable by clicking this field and by nothing else, so someone typing
         // an address could never get to their contacts. A single-line edit gives its focus up on
         // Enter rather than inserting anything, which leaves that key free: it opens the same
         // picker the click does, and the field's description says so.
         let entered = res.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));

         if res.clicked() || entered {
            recipient_selection.open();
         }
      });
   });
}

/// What the recipient field says it holds: how many characters, and the four it ends with.
///
/// The address is on screen, so the node does not repeat it — a reader would hear the whole field
/// back on every keystroke. The ending is what can be checked by ear against the address meant to
/// be pasted, and at four characters or fewer it is the whole string, which is then said as it is.
fn recipient_value(address: &str) -> Option<String> {
   let characters: Vec<char> = address.chars().collect();
   let count = characters.len();

   match count {
      0 => None,
      1..=4 => Some(address.to_string()),
      count => {
         let ending: String = characters[count - 4..].iter().collect();
         Some(format!("{count} characters, ends {ending}"))
      }
   }
}
