//! What an agent asked for and what came of it: the ring the Agents page reads, in its own redb
//! database.
//!
//! One row per entry, keyed by the sequence the entry was recorded under and sealed with the vault-held
//! key, so the store grows by what an agent did rather than by the size of the log: a save writes the
//! rows the file does not have yet and drops the ones the ring has forgotten.
//!
//! The ring is the live copy and the database is its persistence. An action records in memory and
//! touches no file, and the app writes the ring once, when it shuts down -- a log that also rewrote
//! itself on every attempt is a log that writes megabytes as it grows.
//!
//! It is deliberately not the app's log. tracing is redacted for user-identifying data in release builds,
//! and what an agent asked for is exactly that data, so the two never share a channel.

use super::policy::Decision;
use super::profile::{AgentProfile, AgentProfileId};
use crate::WalletStateKey;
use crate::context::RwLockHandle;
use crate::persisted::{PersistedFile, ensure_data_dir, file_path};
use crate::platform::fs::restrict_file_to_owner;
use anyhow::{anyhow, bail};
use redb::{Database as RedbInner, Durability, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::Path;
use zeus_eth::alloy_primitives::{Address, B256};

/// How many entries the ring keeps before the oldest falls off.
///
/// The log answers "what has this thing been doing lately", not "what happened in March": a ring with a
/// fixed ceiling cannot grow without bound on a wallet left running for a year, and an entry older than
/// the ceiling is already in the transaction history the audit points at.
pub const AUDIT_RING_CAPACITY: usize = 5_000;

/// One row per entry: `e` + a big-endian sequence number, so the ring sorts as a range.
const AUDIT_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("audit");

/// The row that says which range of entries the table is the continuation of.
const META_KEY: &[u8] = b"m";

/// redb's default page cache is 1 GiB; the audit store reads and writes a handful of rows at a time.
const PAGE_CACHE_BYTES: usize = 1024 * 1024;

/// On-disk format. Bump when a key or value envelope changes; a newer file is refused.
const AUDIT_DB_FORMAT: u32 = 1;

/// Its own AADs: a shared one is a silent mismatched read across stores.
const AAD_ENTRY: &[u8] = b"zeus-agent-audit-db/entry";
const AAD_META: &[u8] = b"zeus-agent-audit-db/meta";

/// How a shielded withdrawal was broadcast, recorded on the entry it belongs to.
///
/// The two paths are not equally private, so which one ran is part of the record: a self-broadcast names
/// the public wallet as the sender of a transaction that came out of the private pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BroadcastPath {
   /// Submitted from the user's own account.
   SelfBroadcast,
   /// Submitted through the privacy paymaster and its bundler.
   Bundler,
}

/// What the policy decided about one attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditDecision {
   /// Ran without asking.
   Allowed,
   /// Ran, but only after the profile's own mode raised the in-app prompt.
   Prompted,
   /// Refused.
   Denied,
   /// Handed back to the human, who may or may not have finished it.
   NeedsUser,
}

/// One attempted capability: who asked, for what, and what came of it.
///
/// Every field added after the first release carries a default, so an entry written by an older build
/// still loads. Nothing here is a secret and nothing is a full calldata blob -- an entry says what was
/// asked and what moved, never how.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuditEntry {
   /// Unix seconds.
   pub ts: u64,
   /// The profile's id, kept as a string so the entry outlives the profile it names.
   pub profile: AgentProfileId,
   /// The label the profile had at the time, so a renamed profile does not rewrite its past.
   pub profile_label: String,
   /// The tool or flow the request came through.
   pub tool: String,
   /// The capability it needed, whether or not the grant carried it.
   pub capability: String,
   #[serde(default)]
   pub wallet: Option<Address>,
   #[serde(default)]
   pub chain: Option<u64>,
   pub decision: AuditDecision,
   /// The refusal's stable code, when there was one.
   #[serde(default)]
   pub denial: Option<String>,
   /// What the user is told, in one sentence.
   #[serde(default)]
   pub detail: Option<String>,
   /// Estimated value that moved, when the flow could measure it.
   #[serde(default)]
   pub usd_value: Option<f64>,
   /// How an unshield was broadcast, when the action was one.
   #[serde(default)]
   pub broadcast: Option<BroadcastPath>,
   /// The transaction the action produced, so the entry can link to its history row.
   #[serde(default)]
   pub tx_hash: Option<B256>,
   #[serde(default)]
   pub latency_ms: u64,
   /// What the harness called itself, when it said.
   #[serde(default)]
   pub client_info: Option<String>,
}

impl AuditEntry {
   /// An entry for this profile's attempt at a capability, before the outcome is known.
   pub fn new(
      profile: &AgentProfile,
      tool: impl Into<String>,
      capability: impl Into<String>,
      ts: u64,
   ) -> Self {
      Self {
         ts,
         profile: profile.id.clone(),
         profile_label: profile.label.clone(),
         tool: tool.into(),
         capability: capability.into(),
         wallet: None,
         chain: None,
         decision: AuditDecision::Allowed,
         denial: None,
         detail: None,
         usd_value: None,
         broadcast: None,
         tx_hash: None,
         latency_ms: 0,
         client_info: None,
      }
   }

   /// Fill in the decision, and the refusal's own words when there is one.
   pub fn decided(mut self, decision: &Decision) -> Self {
      match decision {
         Decision::Allowed { prompt: true } => self.decision = AuditDecision::Prompted,
         Decision::Allowed { prompt: false } => self.decision = AuditDecision::Allowed,
         Decision::NeedsUser { reason } => {
            self.decision = AuditDecision::NeedsUser;
            self.detail = Some((*reason).to_string());
         }
         Decision::Denied(denial) => {
            self.decision = AuditDecision::Denied;
            self.denial = Some(denial.code().to_string());
            self.detail = Some(denial.to_string());
         }
      }

      self
   }
}

/// What the file holds apart from its rows: the sequence range the rows are the continuation of.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct AuditMeta {
   /// [`AUDIT_DB_FORMAT`] when written; a newer file is refused.
   format: u32,
   /// The sequence of the oldest row in the table.
   first: u64,
   /// One past the newest row: where the next entry goes.
   next: u64,
}

/// What the log holds in memory.
#[derive(Clone, Debug, Default)]
struct AgentAudit {
   /// Oldest first, so the newest end is the tail a reader asks for.
   entries: VecDeque<AuditEntry>,
   /// The sequence the oldest held entry carries: `entries[i]` is `first_seq + i`, which is how a save
   /// tells the rows the file already has from the ones this run added.
   first_seq: u64,
   /// Why a save would replace bytes that did not read. Never persisted: it describes this process's read
   /// of the file, and the next successful load clears it.
   unreadable: Option<String>,
}

impl AgentAudit {
   /// Drop the oldest entries until the ring fits.
   fn trim(&mut self) {
      while self.entries.len() > AUDIT_RING_CAPACITY {
         self.entries.pop_front();
         self.first_seq += 1;
      }
   }
}

/// The newest entries one reader asked for, and how many the ring holds for it.
pub struct Recent {
   /// Newest first, at most the limit the reader asked for.
   pub entries: Vec<AuditEntry>,
   /// How many entries match at any age, so a caller showing twenty rows can still say "of thirty-four".
   pub matching: usize,
}

/// Every agent action this machine recorded, held behind a lock and written to `data/agent_audit.data`.
#[derive(Clone)]
pub struct AgentAuditLog(RwLockHandle<AgentAudit>);

impl Default for AgentAuditLog {
   fn default() -> Self {
      Self::new(AgentAudit::default())
   }
}

impl AgentAuditLog {
   fn new(audit: AgentAudit) -> Self {
      Self(RwLockHandle::new(audit))
   }

   fn read<R>(&self, reader: impl FnOnce(&AgentAudit) -> R) -> R {
      self.0.read(reader)
   }

   fn write<R>(&self, writer: impl FnOnce(&mut AgentAudit) -> R) -> R {
      self.0.write(writer)
   }

   pub fn dir() -> Result<std::path::PathBuf, anyhow::Error> {
      file_path(PersistedFile::AgentAudit)
   }

   pub fn exists() -> Result<bool, anyhow::Error> {
      Ok(Self::dir()?.exists())
   }

   /// Load `data/agent_audit.data`, keeping at most the ring's capacity of entries.
   ///
   /// A missing file is a first run, not a failure: it is an empty log, and the next save writes it.
   pub fn load_from_file(key: &WalletStateKey) -> Result<Self, anyhow::Error> {
      Self::load_from(&Self::dir()?, key)
   }

   /// Test surface: the same load against a path the caller owns.
   pub(crate) fn load_from(path: &Path, key: &WalletStateKey) -> Result<Self, anyhow::Error> {
      let db = match RedbInner::open(path) {
         Ok(db) => db,
         // No file yet: nothing was recorded, so nothing is missing either.
         Err(_) if !path.exists() => return Ok(Self::default()),
         Err(e) => return Err(anyhow!("open the agent audit db: {e}")),
      };

      let read = db.begin_read().map_err(|e| anyhow!("agent audit read txn: {e}"))?;

      let table = match read.open_table(AUDIT_TABLE) {
         Ok(table) => table,
         // A database this build never wrote has no table, and reads as an empty log.
         Err(_) => return Ok(Self::default()),
      };

      let Some(sealed) = table.get(META_KEY).map_err(|e| anyhow!("agent audit get meta: {e}"))?
      else {
         return Ok(Self::default());
      };

      let meta: AuditMeta = key.open_json(sealed.value(), AAD_META)?;

      if meta.format > AUDIT_DB_FORMAT {
         bail!(
            "agent audit db format {} is newer than this build ({AUDIT_DB_FORMAT})",
            meta.format
         );
      }

      let (start, end) = (entry_key(meta.first), entry_key(meta.next));
      let mut entries: VecDeque<AuditEntry> = VecDeque::new();

      for row in table
         .range(start.as_slice()..end.as_slice())
         .map_err(|e| anyhow!("agent audit range: {e}"))?
      {
         let (_, value) = row.map_err(|e| anyhow!("agent audit row: {e}"))?;
         entries.push_back(key.open_json(value.value(), AAD_ENTRY)?);
      }

      // A file written under a larger ceiling still loads here: keep the newest entries it holds.
      let dropped = entries.len().saturating_sub(AUDIT_RING_CAPACITY);
      entries.drain(..dropped);

      Ok(Self::new(AgentAudit {
         entries,
         first_seq: meta.first + dropped as u64,
         unreadable: None,
      }))
   }

   /// Write the ring into `data/agent_audit.data`, adding what the file lacks and dropping what the ring
   /// has forgotten.
   ///
   /// This is the only write: the app calls it once, on shutdown, so a log of every attempt costs one
   /// database write per session rather than one per attempt. An empty ring writes nothing, so an install
   /// that never ran an agent has no file at all.
   ///
   /// Refuses while the file could not be read, for the same reason the profile store does: what is in
   /// memory would then be an empty log, and writing it over the real one would erase the history.
   pub fn save(&self, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      if let Some(reason) = self.unreadable() {
         let path = Self::dir()?;
         return Err(anyhow!(
            "Refusing to overwrite {}: it could not be read ({reason})",
            path.display()
         ));
      }

      if self.is_empty() {
         return Ok(());
      }

      ensure_data_dir()?;
      self.write(|audit| audit.trim());

      self.save_to(&Self::dir()?, key)
   }

   /// Test surface: the same save against a path the caller owns.
   pub(crate) fn save_to(&self, path: &Path, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      if let Some(parent) = path.parent() {
         std::fs::create_dir_all(parent)?;
      }

      let db = RedbInner::builder()
         .set_cache_size(PAGE_CACHE_BYTES)
         .create(path)
         .map_err(|e| anyhow!("open the agent audit db: {e}"))?;

      if let Err(e) = restrict_file_to_owner(path) {
         tracing::warn!("Failed to restrict {}: {e}", path.display());
      }

      let (first, entries) = self.read(|audit| {
         let entries = audit.entries.iter().cloned().collect::<Vec<_>>();
         (audit.first_seq, entries)
      });
      let next = first + entries.len() as u64;

      let mut txn = db.begin_write().map_err(|e| anyhow!("agent audit write txn: {e}"))?;
      txn.set_durability(Durability::Immediate)
         .map_err(|e| anyhow!("agent audit durability: {e}"))?;

      {
         let mut table = txn
            .open_table(AUDIT_TABLE)
            .map_err(|e| anyhow!("agent audit open table: {e}"))?;

         let stored = match table.get(META_KEY).map_err(|e| anyhow!("agent audit get meta: {e}"))? {
            Some(sealed) => Some(key.open_json::<AuditMeta>(sealed.value(), AAD_META)?),
            None => None,
         };

         if let Some(meta) = &stored
            && meta.format > AUDIT_DB_FORMAT
         {
            bail!(
               "agent audit db format {} is newer than this build ({AUDIT_DB_FORMAT})",
               meta.format
            );
         }

         // A file is this ring's continuation while every row it carries is still held at or above the
         // ring's front: its rows are then the ring's unchanged, and only the tail is missing. A first
         // save, or a log this process never loaded, replaces the table instead, so the file can never
         // hold two histories.
         let continuation = stored.as_ref().filter(|meta| meta.first <= first && meta.next <= next);

         match continuation {
            Some(meta) => {
               // The rows the ring dropped fall out of the file; the rows it added are appended.
               for seq in meta.first..meta.next.min(first) {
                  table
                     .remove(entry_key(seq).as_slice())
                     .map_err(|e| anyhow!("agent audit remove: {e}"))?;
               }

               for (offset, entry) in entries.iter().enumerate() {
                  let seq = first + offset as u64;

                  if seq < meta.next {
                     continue;
                  }

                  let sealed = key.seal_json(entry, AAD_ENTRY)?;
                  table
                     .insert(entry_key(seq).as_slice(), sealed.as_slice())
                     .map_err(|e| anyhow!("agent audit insert: {e}"))?;
               }
            }
            None => {
               let stale = table
                  .iter()
                  .map_err(|e| anyhow!("agent audit iter: {e}"))?
                  .map(|row| row.map(|(key, _)| key.value().to_vec()))
                  .collect::<Result<Vec<_>, _>>()
                  .map_err(|e| anyhow!("agent audit row: {e}"))?;

               for row in stale {
                  table.remove(row.as_slice()).map_err(|e| anyhow!("agent audit remove: {e}"))?;
               }

               for (offset, entry) in entries.iter().enumerate() {
                  let sealed = key.seal_json(entry, AAD_ENTRY)?;
                  table
                     .insert(
                        entry_key(first + offset as u64).as_slice(),
                        sealed.as_slice(),
                     )
                     .map_err(|e| anyhow!("agent audit insert: {e}"))?;
               }
            }
         }

         let meta = AuditMeta {
            format: AUDIT_DB_FORMAT,
            first,
            next,
         };

         let sealed = key.seal_json(&meta, AAD_META)?;
         table
            .insert(META_KEY, sealed.as_slice())
            .map_err(|e| anyhow!("agent audit insert meta: {e}"))?;
      }

      txn.commit().map_err(|e| anyhow!("agent audit commit: {e}"))
   }

   /// Record that the file on disk did not read, so no save may replace it.
   pub fn block_saves(&self, reason: impl Into<String>) {
      self.write(|audit| audit.unreadable = Some(reason.into()));
   }

   /// Why saves are blocked, if they are.
   pub fn unreadable(&self) -> Option<String> {
      self.read(|audit| audit.unreadable.clone())
   }

   /// Replace the contents in place after a load, so clones of this handle see the new ones.
   pub fn replace_from(&self, other: &AgentAuditLog) {
      other.read(|source| {
         self.write(|target| *target = source.clone());
      });
   }

   /// Append one entry, dropping the oldest when the ring is full.
   ///
   /// Nothing is written to disk here: the app persists the ring once, on shutdown, through [`Self::save`].
   pub fn record(&self, entry: AuditEntry) {
      self.write(|audit| {
         audit.entries.push_back(entry);
         audit.trim();
      });
   }

   /// The newest limit entries, oldest first.
   pub fn tail(&self, limit: usize) -> Vec<AuditEntry> {
      self.read(|audit| {
         let skip = audit.entries.len().saturating_sub(limit);
         audit.entries.iter().skip(skip).cloned().collect()
      })
   }

   /// Walk back from the newest, keeping the entries `keep` accepts.
   ///
   /// One pass: the count the ring holds for this reader comes out of the same walk as the rows, so
   /// "twenty of thirty-four" never costs a second clone of the log.
   pub fn recent(&self, limit: usize, keep: impl Fn(&AuditEntry) -> bool) -> Recent {
      self.page(0, limit, keep)
   }

   /// One page of the entries `keep` accepts, newest first: the rows that start after the first
   /// `offset` matching entries, and how many there are at any age.
   ///
   /// The walk is what counts them, and only the rows on the page are cloned, so the last page of a
   /// full ring reads twenty entries rather than all five thousand.
   pub fn page(&self, offset: usize, limit: usize, keep: impl Fn(&AuditEntry) -> bool) -> Recent {
      self.read(|audit| {
         let mut matching = 0;
         let mut entries = Vec::new();

         for entry in audit.entries.iter().rev() {
            if !keep(entry) {
               continue;
            }

            matching += 1;

            if matching <= offset || entries.len() >= limit {
               continue;
            }

            entries.push(entry.clone());
         }

         Recent { entries, matching }
      })
   }

   pub fn len(&self) -> usize {
      self.read(|audit| audit.entries.len())
   }

   pub fn is_empty(&self) -> bool {
      self.len() == 0
   }
}

/// `e` + a big-endian sequence: the rows sort oldest first, which is the order the ring reads them.
fn entry_key(seq: u64) -> [u8; 9] {
   let mut key = [0u8; 9];
   key[0] = b'e';
   key[1..].copy_from_slice(&seq.to_be_bytes());
   key
}
