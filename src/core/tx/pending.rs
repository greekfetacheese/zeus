//! Transactions Zeus broadcast but has not seen mined yet.
//!
//! A node accepts a transaction into its mempool and it may not be mined for minutes — or it may be
//! dropped for a fee under the current floor, which is the usual reason a send "fails" without ever
//! reverting. The send path waits [`CLIENT_TIMEOUT_FOR_SENDING_TX`] seconds for a receipt and
//! records nothing if it does not arrive, so a transaction that *was* on its way was reported to the
//! user as an error, left out of the history, and invited a retry that could replace it or
//! double-send with a fresh nonce.
//!
//! This store is that transaction's home in the meantime: the signed bytes exactly as broadcast (so
//! a retry is the same transaction the user confirmed, never a re-signed one at a fee they did not
//! agree to), and the analysis the send path had already computed (so a receipt alone is enough to
//! finish the record, including in a later run, when nothing else survives).
//!
//! It is a sealed file of its own for the same reason as [`crate::core::context::NftDB`]: written on
//! send and rewritten on every re-broadcast, while `wallet_state.data` is re-encrypted on unrelated
//! saves.
//!
//! [`CLIENT_TIMEOUT_FOR_SENDING_TX`]: crate::core::context::CLIENT_TIMEOUT_FOR_SENDING_TX

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use crate::core::WalletStateKey;
use crate::core::context::ZeusCtx;
use crate::core::persisted::{PersistedFile, file_path};
use crate::utils::write_private_atomic;

use super::analysis::TransactionAnalysis;

use zeus_eth::alloy_contract::private::Provider;
use zeus_eth::alloy_network::Ethereum;
use zeus_eth::alloy_primitives::{Bytes, TxHash};
use zeus_eth::alloy_rpc_types::TransactionReceipt;

/// Bound ciphertext to this logical slot (AAD).
const PENDING_TXS_AAD: &[u8] = b"zeus-pending-txs-v1";

/// How long a transaction is left alone before the first re-broadcast.
pub const REBROADCAST_EVERY: Duration = Duration::from_secs(30);
/// How long Zeus keeps re-broadcasting before leaving the transaction to a later run.
pub const GIVE_UP_AFTER: Duration = Duration::from_secs(180);
/// How often the receipt is asked for while waiting.
pub const POLL_INTERVAL: Duration = Duration::from_secs(10);

/// What to do about a broadcast transaction that has no receipt yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryStep {
   /// Nothing yet: it is young enough to be mined on its own.
   Wait,
   /// Send the same signed bytes again — the node may have dropped it from its mempool.
   Rebroadcast,
   /// Stop watching it in this session. The record stays, so a later run picks it up.
   GiveUp,
}

/// The step for a transaction `elapsed` since broadcast with `attempts` re-broadcasts behind it.
///
/// Linear on purpose. Each attempt is one `eth_sendRawTransaction` against the user's own endpoint,
/// and a transaction that has not been mined after six of them is worth leaving to a later run
/// rather than hammering the endpoint for.
pub fn retry_step(elapsed: Duration, attempts: u32) -> RetryStep {
   if elapsed >= GIVE_UP_AFTER {
      return RetryStep::GiveUp;
   }
   match elapsed >= REBROADCAST_EVERY * (attempts + 1) {
      true => RetryStep::Rebroadcast,
      false => RetryStep::Wait,
   }
}

/// A transaction that has been broadcast and not yet seen in a block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingTx {
   pub chain: u64,
   pub hash: TxHash,
   /// The signed transaction as broadcast, so a retry re-sends this and nothing else.
   pub envelope: Bytes,
   /// What the send path computed before broadcasting, so a receipt is all the finalize step needs.
   /// The sender and the interact-to address are in here already.
   pub analysis: TransactionAnalysis,
   /// Whether the flow that sent this knew the user's intent, which decides how the record's main
   /// event is chosen — kept so a resumed transaction records exactly like a live one.
   #[serde(default)]
   pub keep_intent_event: bool,
   /// Unix seconds at broadcast, which is also the timestamp the record will carry.
   pub sent_at: u64,
   /// Re-broadcasts so far.
   #[serde(default)]
   pub attempts: u32,
}

impl PendingTx {
   /// Seconds since it was broadcast.
   pub fn elapsed(&self) -> Duration {
      let now = crate::utils::TimeStamp::now_as_secs()
         .map(|t| t.timestamp().max(0) as u64)
         .unwrap_or(self.sent_at);
      Duration::from_secs(now.saturating_sub(self.sent_at))
   }
}

/// The transactions waiting to be mined. A flat list: it is short, and the hash is inside each
/// record already.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PendingTxStore {
   #[serde(default)]
   pub txs: Vec<PendingTx>,
}

impl PendingTxStore {
   pub fn load_from_file(key: &WalletStateKey) -> Result<Self, anyhow::Error> {
      let sealed = std::fs::read(&Self::dir()?)?;
      let store: Self = key.open_json(&sealed, PENDING_TXS_AAD)?;
      Ok(store)
   }

   pub fn save(&self, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      let sealed = key.seal_json(self, PENDING_TXS_AAD)?;
      write_private_atomic(&Self::dir()?, &sealed)?;
      Ok(())
   }

   pub fn dir() -> Result<std::path::PathBuf, anyhow::Error> {
      file_path(PersistedFile::PendingTxs)
   }

   pub fn exists() -> Result<bool, anyhow::Error> {
      Ok(Self::dir()?.exists())
   }

   /// Remember a freshly broadcast transaction, replacing any record of the same hash.
   pub fn insert(&mut self, tx: PendingTx) {
      self.remove(tx.hash);
      self.txs.push(tx);
   }

   /// Forget one, by hash: it has been mined, or its record has been written.
   pub fn remove(&mut self, hash: TxHash) -> Option<PendingTx> {
      let at = self.txs.iter().position(|tx| tx.hash == hash)?;
      Some(self.txs.remove(at))
   }
}

/// Watch a broadcast transaction until it is mined, re-sending the same signed bytes whenever the
/// node has likely dropped it.
///
/// The bytes are the ones the user confirmed, so a retry can never change what was authorized. A
/// transaction that costs more than they agreed to is their decision to make, not this loop's — a
/// silent fee bump would be a different transaction at a different price.
///
/// Returns the receipt, or an error once the window is up. On the error the record stays in the
/// store, so a later run picks the transaction up instead of losing it.
pub async fn watch_pending_tx<P>(
   ctx: ZeusCtx,
   client: P,
   pending: PendingTx,
) -> Result<TransactionReceipt, anyhow::Error>
where
   P: Provider<Ethereum> + Clone + 'static,
{
   let mut attempts = pending.attempts;
   let started = Instant::now();

   loop {
      if let Some(receipt) = client
         .get_transaction_receipt(pending.hash)
         .await
         .map_err(|e| anyhow!("{:?}", e))?
      {
         return Ok(receipt);
      }

      match retry_step(started.elapsed(), attempts) {
         RetryStep::Wait => {}
         RetryStep::Rebroadcast => {
            attempts += 1;
            ctx.write(|ctx| {
               if let Some(tx) = ctx.pending_txs.txs.iter_mut().find(|tx| tx.hash == pending.hash) {
                  tx.attempts = attempts;
               }
            });
            ctx.save_pending_txs();

            match client.send_raw_transaction(pending.envelope.as_ref()).await {
               Ok(_) => tracing::info!(
                  "Re-broadcast {} (attempt {attempts})",
                  pending.hash
               ),
               Err(e) => tracing::warn!("Re-broadcast of {} failed: {:?}", pending.hash, e),
            }
         }
         RetryStep::GiveUp => {
            return Err(anyhow!(
               "Still pending after {}s and {attempts} re-broadcasts. It stays in your history as \
                pending and Zeus picks it up again on the next start.",
               GIVE_UP_AFTER.as_secs()
            ));
         }
      }

      tokio::time::sleep(POLL_INTERVAL).await;
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   const WRONG_AAD: &[u8] = b"wrong-aad";

   fn pending(hash_byte: u8) -> PendingTx {
      PendingTx {
         chain: 1,
         hash: TxHash::from([hash_byte; 32]),
         envelope: Bytes::from(vec![0x02, 0xf8, 0x6c, 0x01]),
         analysis: TransactionAnalysis::default(),
         keep_intent_event: false,
         sent_at: 1_700_000_000,
         attempts: 0,
      }
   }

   #[test]
   fn the_retry_schedule_rebroadcasts_then_gives_up() {
      // A young transaction is left alone: it may simply be the next block.
      assert_eq!(retry_step(Duration::ZERO, 0), RetryStep::Wait);
      assert_eq!(
         retry_step(REBROADCAST_EVERY - Duration::from_secs(1), 0),
         RetryStep::Wait
      );
      // Then one attempt per interval, and one at a time.
      assert_eq!(
         retry_step(REBROADCAST_EVERY, 0),
         RetryStep::Rebroadcast
      );
      assert_eq!(retry_step(REBROADCAST_EVERY, 1), RetryStep::Wait);
      assert_eq!(
         retry_step(REBROADCAST_EVERY * 2, 1),
         RetryStep::Rebroadcast
      );
      // The window closes on the step that would have re-broadcast it, so giving up wins there; a
      // record left pending is picked up by a later run rather than forgotten.
      assert_eq!(
         retry_step(GIVE_UP_AFTER - Duration::from_secs(1), 5),
         RetryStep::Wait
      );
      assert_eq!(retry_step(GIVE_UP_AFTER, 5), RetryStep::GiveUp);
      assert_eq!(
         retry_step(GIVE_UP_AFTER * 2, 99),
         RetryStep::GiveUp
      );
   }

   #[test]
   fn a_pending_tx_survives_the_seal() {
      let key = WalletStateKey::generate().unwrap();
      let mut store = PendingTxStore::default();
      store.insert(pending(0xaa));
      store.insert(pending(0xbb));

      let sealed = key.seal_json(&store, PENDING_TXS_AAD).unwrap();
      let loaded: PendingTxStore = key.open_json(&sealed, PENDING_TXS_AAD).unwrap();

      assert_eq!(loaded.txs.len(), 2);
      assert_eq!(loaded.txs[0].hash, pending(0xaa).hash);
      assert_eq!(loaded.txs[1].envelope, pending(0xbb).envelope);
      // The bytes are bound to this store's slot, not merely to the key.
      assert!(key.open_json::<PendingTxStore>(&sealed, WRONG_AAD).is_err());
   }

   #[test]
   fn the_same_hash_is_never_recorded_twice() {
      let mut store = PendingTxStore::default();
      store.insert(pending(0xaa));
      store.insert(pending(0xaa));
      assert_eq!(store.txs.len(), 1);

      assert!(store.remove(pending(0xaa).hash).is_some());
      assert!(store.txs.is_empty());
      assert!(store.remove(pending(0xaa).hash).is_none());
   }
}
