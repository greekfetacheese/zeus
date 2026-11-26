//! What an agent asked for and what came of it: the ring the Agents page reads, sealed on disk.
//!
//! Shaped like the profile store beside it -- a private lock, its own AAD, write_private_atomic on the way
//! out, and a load failure that blocks the next save -- with one difference that matters: this is an
//! append-only ring, not a document that gets edited. A refusal is an entry too, because "the agent tried
//! to send outside its allowlist" is the most important line a user can be shown.
//!
//! It is deliberately not the app's log. tracing is redacted for user-identifying data in release builds,
//! and what an agent asked for is exactly that data, so the two never share a channel.

use super::policy::Decision;
use super::profile::{AgentProfile, AgentProfileId};
use crate::WalletStateKey;
use crate::context::RwLockHandle;
use crate::persisted::{PersistedFile, ensure_data_dir, file_path};
use crate::platform::fs::write_private_atomic;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use zeus_eth::alloy_primitives::{Address, B256};

/// Its own AAD: a shared one is a silent mismatched read across stores.
const AGENT_AUDIT_AAD: &[u8] = b"zeus-agent-audit-v1";

/// How many entries the ring keeps before the oldest falls off.
///
/// The log answers "what has this thing been doing lately", not "what happened in March": a ring with a
/// fixed ceiling cannot grow without bound on a wallet left running for a year, and an entry older than
/// the ceiling is already in the transaction history the audit points at.
pub const AUDIT_RING_CAPACITY: usize = 5_000;

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

/// What the file holds.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct AgentAudit {
   /// Oldest first, so the newest end is the tail a reader asks for.
   #[serde(default)]
   entries: VecDeque<AuditEntry>,
   /// Why a save would replace bytes that did not read. Never persisted: it describes this process's read
   /// of the file, and the next successful load clears it.
   #[serde(skip)]
   unreadable: Option<String>,
}

impl AgentAudit {
   /// Drop the oldest entries until the ring fits.
   fn trim(&mut self) {
      while self.entries.len() > AUDIT_RING_CAPACITY {
         self.entries.pop_front();
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

/// Every agent action this machine recorded, held behind a lock and sealed on disk.
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

   /// Seal the log without writing it. Pair with decode.
   pub fn encode(&self, key: &WalletStateKey) -> Result<Vec<u8>, anyhow::Error> {
      self.read(|audit| key.seal_json(audit, AGENT_AUDIT_AAD))
   }

   /// Open sealed bytes, keeping at most the ring's capacity of entries.
   pub fn decode(key: &WalletStateKey, sealed: &[u8]) -> Result<Self, anyhow::Error> {
      let mut audit: AgentAudit = key.open_json(sealed, AGENT_AUDIT_AAD)?;
      audit.trim();
      Ok(Self::new(audit))
   }

   pub fn load_from_file(key: &WalletStateKey) -> Result<Self, anyhow::Error> {
      let sealed = std::fs::read(Self::dir()?)?;
      Self::decode(key, &sealed)
   }

   /// Seal and write the log.
   ///
   /// Refuses while the file could not be read, for the same reason the profile store does: what is in
   /// memory would then be an empty log, and sealing it over the real one would erase the history.
   pub fn save(&self, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      if let Some(reason) = self.unreadable() {
         let path = Self::dir()?;
         return Err(anyhow!(
            "Refusing to overwrite {}: it could not be read ({reason})",
            path.display()
         ));
      }

      ensure_data_dir()?;
      self.write(|audit| audit.trim());

      let sealed = self.encode(key)?;
      write_private_atomic(&Self::dir()?, &sealed)
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
   /// Nothing is written to disk here: a caller that wants the entry persisted calls save with the
   /// vault's key, which is the same division the profile store has.
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
      self.read(|audit| {
         let mut matching = 0;
         let mut entries = Vec::new();

         for entry in audit.entries.iter().rev() {
            if !keep(entry) {
               continue;
            }

            matching += 1;

            if entries.len() < limit {
               entries.push(entry.clone());
            }
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
