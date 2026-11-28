//! The `contacts` screen: the address book, as three list operations.
//!
//! A contact is a row, not a set of fields. A patch that replaced the whole list would need the caller
//! to send back everything it read, and a row lost in that round trip is a label the user has to type
//! again — so the patch is `add`, `rename` and `remove`, each one applied through the store's own
//! contact functions, which is where the uniqueness rules already live.
//!
//! Addresses are matched by what they *parse* to, not by the text: the app has always stored whatever
//! spelling a contact arrived with, so `0xAbC…` and `0xabc…` are the same contact here and nowhere
//! else.

use super::{Applied, Closed, SettingsError, SettingsScreen, deserialize, refuse_closed};
use crate::ZeusCtx;
use crate::types::Contact;
use serde::Deserialize;
use serde_json::{Value, json};
use std::str::FromStr;
use zeus_eth::alloy_primitives::Address;

/// One patch of the contacts screen.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Patch {
   add: Option<Vec<New>>,
   rename: Option<Vec<Rename>>,
   remove: Option<Vec<String>>,
}

/// One contact to add.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct New {
   name: String,
   address: String,
   #[serde(default)]
   zk_address: Option<String>,
}

/// One contact to rename, found by its address.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Rename {
   address: String,
   name: String,
}

/// Nothing on this screen is out of reach: what an agent may not do is *change a contact's address*,
/// and the patch has no field for that at all.
const CLOSED: &[Closed] = &[];

pub const SCREEN: SettingsScreen = SettingsScreen {
   name: "contacts",
   capability: "settings.contacts.write",
   schema,
   closed: CLOSED,
   write,
};

fn schema() -> Value {
   json!({
       "type": "object",
       "additionalProperties": false,
       "properties": {
           "add": {
               "type": "array",
               "persisted": true,
               "description": "Contacts to add. A name and an address are required; a zk address is optional.",
               "items": {
                   "type": "object",
                   "additionalProperties": false,
                   "properties": {
                       "name": {"type": "string"},
                       "address": {"type": "string"},
                       "zk_address": {"type": "string"},
                   },
                   "required": ["name", "address"],
               },
           },
           "rename": {
               "type": "array",
               "persisted": true,
               "description": "Contacts to rename, each found by its address.",
               "items": {
                   "type": "object",
                   "additionalProperties": false,
                   "properties": {
                       "address": {"type": "string"},
                       "name": {"type": "string"},
                   },
                   "required": ["address", "name"],
               },
           },
           "remove": {
               "type": "array",
               "persisted": true,
               "description": "Addresses to remove. An address that names no contact is refused rather than ignored.",
               "items": {"type": "string"},
           },
       },
   })
}

fn write(ctx: &ZeusCtx, value: &Value) -> Result<Applied, SettingsError> {
   let patch = check(value)?;
   let applied = apply(ctx, patch)?;

   if applied.changed.is_empty() {
      return Ok(applied);
   }

   ctx.save_wallet_state()
      .map_err(|error| SettingsError::refused(error.to_string()))?;

   Ok(applied)
}

pub(super) fn check(value: &Value) -> Result<Patch, SettingsError> {
   refuse_closed(value, CLOSED)?;
   deserialize(value, "contacts")
}

pub(super) fn apply(ctx: &ZeusCtx, patch: Patch) -> Result<Applied, SettingsError> {
   let mut changed: Vec<&'static str> = Vec::new();

   for entry in patch.add.unwrap_or_default() {
      let address = address(&entry.address)?;
      let name = name(&entry.name)?;
      ensure_new(ctx, address, &name)?;

      let contact = Contact::new(
         name,
         format!("{address:#x}"),
         entry.zk_address.unwrap_or_default(),
      );

      ctx.add_contact(contact)
         .map_err(|error| SettingsError::invalid(error.to_string()))?;
      note(&mut changed, "add");
   }

   for entry in patch.rename.unwrap_or_default() {
      let stored = stored(ctx, &entry.address)?;
      let name = name(&entry.name)?;

      ctx.rename_contact(&stored, name)
         .map_err(|error| SettingsError::invalid(error.to_string()))?;
      note(&mut changed, "rename");
   }

   for text in patch.remove.unwrap_or_default() {
      let stored = stored(ctx, &text)?;
      ctx.remove_contact(&stored);
      note(&mut changed, "remove");
   }

   Ok(Applied { changed })
}

fn address(text: &str) -> Result<Address, SettingsError> {
   Address::from_str(text).map_err(|_| SettingsError::invalid(format!("not an address: {text}")))
}

fn name(text: &str) -> Result<String, SettingsError> {
   let trimmed = text.trim();

   if trimmed.is_empty() {
      return Err(SettingsError::invalid("a contact needs a name"));
   }

   Ok(trimmed.to_owned())
}

/// The spelling a stored contact is filed under, found by parsing both sides.
fn stored(ctx: &ZeusCtx, text: &str) -> Result<String, SettingsError> {
   let wanted = address(text)?;

   ctx.contacts()
      .into_iter()
      .find(|contact| Address::from_str(&contact.evm_address).is_ok_and(|stored| stored == wanted))
      .map(|contact| contact.evm_address)
      .ok_or_else(|| SettingsError::invalid(format!("no contact with address {text}")))
}

/// Refuse an addition the store would refuse, with the field named before anything is written.
///
/// The store compares addresses as text, and a contact can be stored with any spelling, so the check
/// here parses both sides: a second row for the same address would otherwise slip past it.
fn ensure_new(ctx: &ZeusCtx, address: Address, name: &str) -> Result<(), SettingsError> {
   for contact in ctx.contacts() {
      if contact.name == name {
         return Err(SettingsError::invalid(format!(
            "a contact named {name} already exists"
         )));
      }

      if Address::from_str(&contact.evm_address).is_ok_and(|stored| stored == address) {
         return Err(SettingsError::invalid(format!(
            "a contact at {address:#x} already exists"
         )));
      }
   }

   Ok(())
}

/// Name a field once, however many entries of it the patch carried.
fn note(changed: &mut Vec<&'static str>, field: &'static str) {
   if !changed.contains(&field) {
      changed.push(field);
   }
}
