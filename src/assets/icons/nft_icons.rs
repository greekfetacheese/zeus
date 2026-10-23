use super::NftKey;
use super::disk;
use super::load_image;
use crate::core::context::handle::RwLockHandle;
use eframe::egui::Context;
use eframe::egui::TextureHandle;
use eframe::egui::epaint::textures::TextureOptions;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use zeus_eth::alloy_primitives::Address;
use zeus_eth::alloy_primitives::U256;

/// Stored art for one NFT: the two raster renderings a view asks for.
///
/// One shape for every kind of art. A grid and a detail view ask for different sizes, so the
/// renderings are produced once at ingest — art is never kept as source, because a renderer handed
/// source resolves `href` against the filesystem, which turns a collection's `tokenURI` into a
/// local file read. See [`crate::utils::nft_icon`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NftIconData {
   /// The list thumbnail.
   pub x64: Vec<u8>,
   /// The copy kept for inspecting a single NFT.
   pub x250: Vec<u8>,
   /// The metadata URI this art was read from, when it is known.
   ///
   /// The cache key is `(collection, token id)`, so this is the only way to notice that the URI behind
   /// the art changed — a reveal, an upgrade. See [`NftIcons::reconcile_art`].
   pub source_uri: Option<String>,
}

impl NftIconData {
   /// Whether this holds anything worth showing — a half-written directory holds nothing.
   pub fn is_empty(&self) -> bool {
      self.x64.is_empty() && self.x250.is_empty()
   }
}

/// The bytes held by an in-memory art map: both renderings per entry, plus the metadata URI each art
/// was read from.
fn art_bytes(cache: &HashMap<NftKey, NftIconData>) -> usize {
   cache
      .values()
      .map(|data| {
         data.x64.len() + data.x250.len() + data.source_uri.as_ref().map_or(0, String::len)
      })
      .sum()
}

/// How many tokens' art is held in memory.
///
/// The in-memory copy is the hot one and the disk copy is the durable one
/// ([`disk::MAX_DISK_BYTES`]), so these two bounds are backstops against pathological growth rather
/// than a working-set limit: past them art is dropped from memory and read back from disk when it is
/// next asked for, which is never a re-download.
const MAX_ART_ENTRIES: usize = 512;

/// The byte budget for that in-memory copy, counted over everything an entry holds ([`art_bytes`]): the
/// two renderings **and** the `source_uri` metadata document they came from.
///
/// Counting the URI is what makes this bound the whole map. A URI is contract-controlled, and its
/// length is not bounded here — a `data:` URI *is* its payload, so it is bounded only by the transfer
/// cap, `nft_icon::MAX_URI_LEN` (16 MiB). Leaving it out of the count put the real resident ceiling at
/// this budget *plus* up to [`MAX_ART_ENTRIES`] × that cap; counting it holds the map to this budget,
/// whatever shape the URIs take. A URI that only *points* at a document is bounded far tighter where
/// it is read, at `zeus_eth::nft::MAX_EXPANDED_URI_LEN` (64 KiB).
const MAX_ART_BYTES: usize = 64 * 1024 * 1024;

/// Downloaded NFT images.
///
/// Keyed by collection **and** token id, because one collection holds many tokens — an address
/// alone would give every token in a collection the first token's picture. Unlike token icons there
/// is no baked-in set: every NFT image comes from the chain or not at all.
pub struct NftIcons {
   icons_x64: RwLockHandle<HashMap<NftKey, TextureHandle>>,
   icons_x250: RwLockHandle<HashMap<NftKey, TextureHandle>>,
   /// Raw PNG bytes. Kept so textures are decompressed and uploaded only when a view asks.
   icon_data: RwLockHandle<HashMap<NftKey, NftIconData>>,
   /// The same keys, oldest touched first, so [`NftIcons::evict_art`] knows what to drop.
   ///
   /// Touched on insert rather than on every read: art is fetched in the order it is looked at, so the
   /// two orders agree without taking a write lock on the frame path.
   art_order: RwLockHandle<VecDeque<NftKey>>,
   /// In-flight downloads, so two views asking at once do not fetch twice.
   in_flight: RwLockHandle<HashSet<NftKey>>,
   /// Tokens whose image was missing this session — don't retry until restart.
   failed: RwLockHandle<HashSet<NftKey>>,
   egui_ctx: Context,
   /// Shown when a token has no image, or its download has not finished yet.
   pub placeholder: TextureHandle,
}

impl Default for NftIcons {
   fn default() -> Self {
      let egui_ctx = Context::default();
      Self::new(&egui_ctx).unwrap()
   }
}

impl NftIcons {
   pub fn new(ctx: &Context) -> Result<Self, anyhow::Error> {
      let placeholder = load_image(include_bytes!("nft/placeholder.png"))?;
      let placeholder = ctx.load_texture(
         "nft_placeholder",
         placeholder,
         TextureOptions::default(),
      );

      // Images downloaded in previous sessions — the newest [`MAX_ART_ENTRIES`] of them; the rest stay
      // on disk and are read back when a view asks.
      let loaded = disk::load_downloaded_nft_icons(MAX_ART_ENTRIES);
      let mut art_order: VecDeque<NftKey> = VecDeque::with_capacity(loaded.len());
      let mut icon_data: HashMap<NftKey, NftIconData> = HashMap::with_capacity(loaded.len());
      for (key, data) in loaded {
         art_order.push_back(key);
         icon_data.insert(key, data);
      }

      let icons = Self {
         icons_x64: RwLockHandle::new(HashMap::new()),
         icons_x250: RwLockHandle::new(HashMap::new()),
         icon_data: RwLockHandle::new(icon_data),
         art_order: RwLockHandle::new(art_order),
         in_flight: RwLockHandle::new(HashSet::new()),
         failed: RwLockHandle::new(HashSet::new()),
         egui_ctx: ctx.clone(),
         placeholder,
      };

      // A startup load is capped by entries, never by bytes — and disk can hold what a build from before
      // the byte bound was added wrote. Enforcing it here is what keeps the in-memory copy inside it.
      icons.evict_art();

      Ok(icons)
   }

   /// The raster texture for one rendering, if the stored art is raster.
   ///
   /// Falls back to the other rendering rather than to the placeholder: a 250px copy shown in a
   /// 64px row is still the right picture.
   pub(super) fn raster_texture(&self, key: &NftKey, large: bool) -> Option<TextureHandle> {
      let cache = if large {
         &self.icons_x250
      } else {
         &self.icons_x64
      };

      if let Some(handle) = cache.read(|cache| cache.get(key).cloned()) {
         return Some(handle);
      }

      // The `None` for "no art for this key" is decided here, outside the lock.
      let bytes = self.icon_data.read(|icon_data| {
         icon_data.get(key).map(|data| {
            let (preferred, other) = if large {
               (&data.x250, &data.x64)
            } else {
               (&data.x64, &data.x250)
            };
            if preferred.is_empty() {
               other.clone()
            } else {
               preferred.clone()
            }
         })
      });

      let bytes = bytes?;

      if bytes.is_empty() {
         return None;
      }

      match load_image(&bytes) {
         Ok(image) => {
            let size = if large { 250 } else { 64 };
            let name = format!("nft{}_{}_{}_{}", size, key.0, key.1, key.2);
            let handle = self.egui_ctx.load_texture(name, image, TextureOptions::default());
            cache.write(|cache| cache.insert(*key, handle.clone()));
            Some(handle)
         }
         Err(_e) => {
            #[cfg(feature = "dev")]
            tracing::warn!("Failed to decode NFT image for {}: {}", key.0, _e);
            None
         }
      }
   }

   pub fn has_icon(&self, key: &NftKey) -> bool {
      self
         .icon_data
         .read(|icon_data| icon_data.get(key).is_some_and(|data| !data.is_empty()))
   }

   /// A token id of `collection` that has art cached, if any. Lowest id wins, so the choice is stable
   /// across frames rather than whatever the map happens to iterate first.
   ///
   /// For rows that are about a collection rather than one token — an approval log carries no token id —
   /// where asking for a fixed id (0, say) can land on a token whose art was never fetched.
   pub(super) fn cached_id_for_collection(
      &self,
      chain_id: u64,
      collection: Address,
   ) -> Option<U256> {
      self.icon_data.read(|icon_data| {
         icon_data
            .iter()
            .filter(|(key, data)| key.0 == collection && key.1 == chain_id && !data.is_empty())
            .map(|(key, _)| key.2)
            .min()
      })
   }

   /// Whether this token still needs a download attempt.
   ///
   /// Not quite read-only: art that is on disk but was evicted from the bounded in-memory copy is read
   /// back here, so a caller counting the fetches it starts does not spend a slot on one that has
   /// nothing left to do. Both callers are off the frame path.
   pub fn needs_fetch(&self, key: &NftKey) -> bool {
      !self.hydrate(key)
         && !self.in_flight.read(|in_flight| in_flight.contains(key))
         && !self.failed.read(|failed| failed.contains(key))
   }

   pub fn insert_icon(&self, key: NftKey, data: NftIconData) {
      self.icon_data.write(|icon_data| icon_data.insert(key, data));
      self.touch(&key);
      self.evict_art();
   }

   /// Record `key` as the most recently touched, without duplicating it in the order list.
   fn touch(&self, key: &NftKey) {
      self.art_order.write(|order| {
         if let Some(at) = order.iter().position(|existing| existing == key) {
            order.remove(at);
         }
         order.push_back(*key);
      });
   }

   /// Keep the in-memory copy inside its budget, dropping the least recently touched art first.
   ///
   /// Only the memory copy: the disk copy is the durable one, and [`NftIcons::hydrate`] reads it back, so
   /// an eviction costs a file read rather than a download. A stale purge or a failed fetch may already
   /// have removed a key from the map, in which case its place in the order list is simply skipped.
   fn evict_art(&self) {
      self.art_order.write(|order| {
         self.icon_data.write(|cache| {
            while cache.len() > MAX_ART_ENTRIES || art_bytes(cache) > MAX_ART_BYTES {
               let Some(oldest) = order.pop_front() else {
                  break;
               };
               if cache.remove(&oldest).is_some() {
                  self.icons_x64.write(|icons_x64| icons_x64.remove(&oldest));
                  self.icons_x250.write(|icons_x250| icons_x250.remove(&oldest));
               }
            }
         });
      });
   }

   /// Bring art that is on disk but not in memory back into memory. `true` when there is art now.
   fn hydrate(&self, key: &NftKey) -> bool {
      if self.has_icon(key) {
         return true;
      }

      let Some(data) = disk::load_nft_icon(key.1, key.0, key.2) else {
         return false;
      };

      self.insert_icon(*key, data);
      true
   }

   /// Bring the cache's record of `key`'s art in line with the URI the token reports now.
   ///
   /// The cache is keyed by `(collection, token id)`, so without this a collection that changes its
   /// `tokenURI` — a reveal, an upgrade — would keep showing the old picture for good. Three cases:
   ///
   /// - Both URIs are known and differ: the art is dropped, from memory and from disk, so the next fetch
   ///   reads the new one.
   /// - The stored URI is unknown — art cached by a version that recorded none: the current URI becomes
   ///   the baseline. Its bytes cannot be checked retroactively, and re-downloading every user's art on
   ///   upgrade to buy nothing would be worse than starting to compare from here.
   /// - The current URI is unknown (the contract exposes no metadata, or the token has no art cached):
   ///   nothing to compare, so nothing happens. Art for a token whose metadata went away is still that
   ///   token's art.
   pub fn reconcile_art(&self, key: &NftKey, current: Option<&str>) {
      let Some(current) = current else {
         return;
      };

      let Some(stored) =
         self.icon_data.read(|cache| cache.get(key).map(|data| data.source_uri.clone()))
      else {
         return;
      };

      match stored {
         Some(stored) if stored == current => {}
         Some(_) => {
            #[cfg(feature = "dev")]
            tracing::debug!(
               "NFT art for {} is from an older metadata URI, refetching",
               key.0
            );
            self.remove_icon(key);
            if let Err(_e) = disk::delete_nft_icon(key.1, key.0, key.2) {
               #[cfg(feature = "dev")]
               tracing::warn!("Failed to drop stale NFT art for {}: {_e}", key.0);
            }
            // The re-fetch is a real download, so it starts from a clean slate rather than the session's
            // "already tried and missing" set.
            self.failed.write(|failed| failed.remove(key));
         }
         None => {
            if let Err(_e) = disk::save_nft_icon_source(key.1, key.0, key.2, current) {
               #[cfg(feature = "dev")]
               tracing::debug!(
                  "Failed to record the source URI for {}: {_e}",
                  key.0
               );
               return;
            }
            self.icon_data.write(|icon_data| {
               if let Some(data) = icon_data.get_mut(key) {
                  data.source_uri = Some(current.to_string());
               }
            });
         }
      }
   }

   pub fn remove_icon(&self, key: &NftKey) {
      self.icon_data.write(|icon_data| icon_data.remove(key));
      self.icons_x64.write(|icons_x64| icons_x64.remove(key));
      self.icons_x250.write(|icons_x250| icons_x250.remove(key));
   }

   /// Mark a download as started. Returns false when we already have the image, one is in flight, or it
   /// came back missing this session.
   ///
   /// "Already have" includes art that is on disk but was evicted from memory: that is read back rather
   /// than downloaded again.
   pub fn try_begin_fetch(&self, key: &NftKey) -> bool {
      if self.hydrate(key) {
         return false;
      }
      if self.failed.read(|failed| failed.contains(key)) {
         return false;
      }
      self.in_flight.write(|in_flight| in_flight.insert(*key))
   }

   pub fn finish_fetch(&self, key: &NftKey, not_found: bool) {
      self.in_flight.write(|in_flight| in_flight.remove(key));
      if not_found {
         self.failed.write(|failed| failed.insert(*key));
      }
   }
}

#[cfg(test)]
mod tests {
   use super::{MAX_ART_BYTES, MAX_ART_ENTRIES, NftIconData, NftIcons, art_bytes};
   use crate::assets::icons::Icons;
   use crate::assets::icons::NftKey;
   use crate::assets::icons::disk;
   use eframe::egui::Context;
   use zeus_eth::alloy_primitives::Address;
   use zeus_eth::alloy_primitives::U256;

   const PLACEHOLDER: &[u8] = include_bytes!("nft/placeholder.png");

   /// A collection-wide row — an approval — takes its art from whichever of the collection's tokens has
   /// some cached. Asking for a fixed id, id 0, showed the placeholder whenever that particular token
   /// had never been fetched, which is exactly what an NFT approval looked like.
   #[test]
   fn a_collection_icon_comes_from_any_cached_token() {
      let ctx = Context::default();
      let icons = Icons::new(&ctx).expect("the bundled icons load");

      let collection = Address::from([0xbc; 20]);

      // Nothing cached: no token of this collection has art to lend.
      assert_eq!(
         icons.nfts.cached_id_for_collection(1, collection),
         None,
         "nothing cached yet"
      );

      // Art cached under a token id that is not 0.
      let key = (collection, 1, U256::from(7));
      icons.nfts.insert_icon(
         key,
         raster(PLACEHOLDER.to_vec(), PLACEHOLDER.to_vec()),
      );

      assert_eq!(
         icons.nfts.cached_id_for_collection(1, collection),
         Some(U256::from(7)),
         "the id-less row takes whichever token is cached, not a fixed id"
      );

      // Drawing that row uploads the art under the cached token's key. `.uri()` used to be what said
      // which art a row got; a texture has no URI of its own to read instead, so read the cache.
      let _ = icons.nft_collection_icon_x64(1, collection, false);
      assert!(
         icons.nfts.icons_x64.read(|c| c.get(&key).is_some()),
         "and that art is what the row then shows"
      );

      // Another chain does not see it: the art is keyed per chain.
      assert_eq!(
         icons.nfts.cached_id_for_collection(10, collection),
         None
      );
   }

   /// A row that names a token never borrows its collection's art.
   ///
   /// A mint names an id that does not exist yet, so the placeholder is right there — and showing the art
   /// of a *sibling* token under this row's id would be showing a different NFT. Only the rows about a
   /// collection as a whole (an approval log, which carries no id) take any cached art of it.
   #[test]
   fn a_tokens_row_does_not_borrow_the_collections_art() {
      let ctx = Context::default();
      let icons = Icons::new(&ctx).expect("the bundled icons load");

      let collection = Address::from([0xaf; 20]);
      let chain_id = 11155111;

      // Art cached for a token of the collection that is not the one being drawn.
      let cached = (collection, chain_id, U256::from(500));
      icons.nfts.insert_icon(
         cached,
         raster(PLACEHOLDER.to_vec(), PLACEHOLDER.to_vec()),
      );

      // A row for another token of the collection: it falls to the placeholder, and no art of the
      // sibling's is left cached under its own key.
      let other = (collection, chain_id, U256::from(1521));
      let _ = icons.nft_icon_x64(chain_id, collection, other.2, false);
      assert!(
         icons.nfts.icons_x64.read(|c| c.get(&other).is_none()),
         "a token's row must not show another token's art"
      );

      // The collection's own row — the id-less one — is where that cached art shows up.
      let _ = icons.nft_collection_icon_x64(chain_id, collection, false);
      assert!(
         icons.nfts.icons_x64.read(|c| c.get(&cached).is_some()),
         "the id-less row is the one that takes the collection's art"
      );
   }

   /// Shorthand for an entry with both renderings. There is no `Default`, because an empty entry is
   /// not something to build by accident.
   fn raster(x64: Vec<u8>, x250: Vec<u8>) -> NftIconData {
      NftIconData {
         x64,
         x250,
         source_uri: None,
      }
   }

   /// Art whose metadata URI changed is dropped, so the next fetch reads the new one.
   ///
   /// A reveal is exactly this: the same collection and token id with a different `tokenURI`. The cache is
   /// keyed by that pair, so without the check the old picture outlives the reveal — for good.
   #[test]
   fn art_from_an_older_metadata_uri_is_dropped() {
      let ctx = Context::default();
      let icons = Icons::new(&ctx).expect("the bundled icons load");
      let key = (Address::from([0x22; 20]), 1, U256::from(3));

      let mut data = raster(vec![1, 2], vec![3, 4]);
      data.source_uri = Some("ipfs://cid/3.json".to_string());
      icons.nfts.insert_icon(key, data);
      assert!(icons.nfts.has_icon(&key), "cached");

      // The same URI: nothing to do.
      icons.nfts.reconcile_art(&key, Some("ipfs://cid/3.json"));
      assert!(
         icons.nfts.has_icon(&key),
         "an unchanged URI keeps the art"
      );

      // A different one: the art goes, and the fetch path now has something to do.
      icons.nfts.reconcile_art(&key, Some("https://revealed.example/3.json"));
      assert!(
         !icons.nfts.has_icon(&key),
         "art from an older metadata URI must not outlive the reveal"
      );
      assert!(
         icons.nfts.needs_fetch(&key),
         "and the token is back to needing a fetch"
      );
   }

   /// Art cached before the URI was recorded adopts the current one instead of being re-downloaded.
   ///
   /// Those bytes cannot be compared against anything retroactively, and throwing them away would
   /// re-download every user's art on upgrade to buy nothing. The current URI becomes the baseline a
   /// later change is measured from.
   #[test]
   fn art_without_a_recorded_uri_adopts_the_current_one() {
      let ctx = Context::default();
      let icons = Icons::new(&ctx).expect("the bundled icons load");
      let key = (Address::from([0x33; 20]), 1, U256::from(4));

      icons.nfts.insert_icon(key, raster(vec![1], vec![2]));

      icons.nfts.reconcile_art(&key, Some("ipfs://cid/4.json"));

      assert!(
         icons.nfts.has_icon(&key),
         "the art is kept, not re-fetched"
      );
      assert_eq!(
         icons.nfts.icon_data.read(|c| c[&key].source_uri.clone()),
         Some("ipfs://cid/4.json".to_string()),
         "with the current URI recorded as the baseline"
      );

      // The adopt writes a sidecar into the real data directory; leave it as it was found.
      let _ = disk::delete_nft_icon(key.1, key.0, key.2);
   }

   /// The in-memory copy is bounded, and the art it drops is the oldest touched.
   ///
   /// The bound is what keeps a wallet's art from being "as much memory as the user has looked at", and it
   /// is safe to hit because the disk copy is the durable one: what is dropped here is read back from disk
   /// when it is next asked for, not downloaded again.
   #[test]
   fn the_in_memory_art_cache_drops_the_oldest_past_its_bound() {
      let ctx = Context::default();
      let icons = Icons::new(&ctx).expect("the bundled icons load");
      let collection = Address::from([0x11; 20]);

      let keys: Vec<NftKey> = (0..MAX_ART_ENTRIES as u64 + 3)
         .map(|id| (collection, 1, U256::from(id)))
         .collect();

      for key in &keys {
         icons.nfts.insert_icon(*key, raster(vec![1, 2, 3, 4], vec![5, 6]));
      }

      assert!(
         icons.nfts.icon_data.read(|c| c.len()) <= MAX_ART_ENTRIES,
         "the map stays inside its entry bound"
      );
      assert!(
         !icons.nfts.has_icon(&keys[0]) && !icons.nfts.has_icon(&keys[2]),
         "the oldest art is what went"
      );
      assert!(
         icons.nfts.has_icon(&keys[keys.len() - 1]),
         "and the newest survives"
      );
   }

   /// The in-memory budget counts the metadata URI as well as the renderings.
   ///
   /// `art_bytes` counted only `x64` and `x250`, while each entry's `source_uri` is contract-controlled
   /// and bounded only where it is read — so the real resident ceiling was `MAX_ART_BYTES` *plus* up to
   /// `MAX_ART_ENTRIES` URIs, which the constant's own comment spelled out. Counting the URI makes
   /// `MAX_ART_BYTES` the whole map's budget, and the eviction decision reads it, so an entry that is
   /// over budget only because of its URI is dropped like any other.
   #[test]
   fn the_art_byte_budget_counts_the_source_uri() {
      let ctx = Context::default();
      let icons = Icons::new(&ctx).expect("the bundled icons load");
      let collection = Address::from([0x44; 20]);
      let key = (collection, 1, U256::from(1));

      let mut data = raster(vec![1, 2], vec![3, 4]);
      data.source_uri = Some("x".repeat(MAX_ART_BYTES + 1));

      icons.nfts.icon_data.write(|c| c.insert(key, data));
      assert!(
         icons.nfts.icon_data.read(art_bytes) > MAX_ART_BYTES,
         "the URI's bytes must count against the budget"
      );

      icons.nfts.art_order.write(|c| c.push_back(key));
      icons.nfts.evict_art();

      assert!(
         icons.nfts.icon_data.read(|c| c.is_empty()),
         "an entry over budget because of its URI is evicted"
      );
   }

   #[test]
   fn nft_icon_data_knows_when_it_is_empty() {
      assert!(raster(Vec::new(), Vec::new()).is_empty());
      assert!(
         !raster(vec![1], Vec::new()).is_empty(),
         "one rendering is enough to show"
      );
      assert!(
         !raster(Vec::new(), vec![2]).is_empty(),
         "either rendering is enough to show"
      );
   }

   /// The dedupe contract the fetch path relies on: one download per token, and a miss is not
   /// retried for the rest of the session.
   #[test]
   fn fetch_dedupe_lifecycle() {
      let icons = NftIcons::default();
      let key = (Address::from([0xbc; 20]), 1, U256::from(1));

      assert!(
         icons.try_begin_fetch(&key),
         "the first ask starts a fetch"
      );
      assert!(
         !icons.try_begin_fetch(&key),
         "a second ask while one is in flight must be refused"
      );

      icons.finish_fetch(&key, true);
      assert!(
         !icons.try_begin_fetch(&key),
         "a miss must not be retried this session"
      );

      icons.insert_icon(
         key,
         raster(PLACEHOLDER.to_vec(), PLACEHOLDER.to_vec()),
      );
      assert!(icons.has_icon(&key));
      assert!(
         !icons.try_begin_fetch(&key),
         "an image we already have needs no fetch"
      );

      // The token id is part of the key: a sibling token in the same collection is independent.
      let sibling = (key.0, 1, U256::from(2));
      assert!(icons.try_begin_fetch(&sibling));

      icons.remove_icon(&key);
      assert!(!icons.has_icon(&key));
   }

   /// A missing rendering falls back to the other one instead of dropping to the placeholder.
   #[test]
   fn a_missing_rendering_falls_back_to_the_other() {
      let icons = NftIcons::default();

      let large_only = (Address::from([0xbc; 20]), 1, U256::from(9));
      icons.insert_icon(
         large_only,
         raster(Vec::new(), PLACEHOLDER.to_vec()),
      );
      assert!(
         icons.raster_texture(&large_only, false).is_some(),
         "a thumbnail request must still get the 250px copy"
      );
      assert!(icons.raster_texture(&large_only, true).is_some());

      // Empty data is not an icon: the caller must be told to show the placeholder.
      let empty = (Address::from([0xbc; 20]), 1, U256::from(10));
      icons.insert_icon(empty, raster(Vec::new(), Vec::new()));
      assert!(icons.raster_texture(&empty, false).is_none());
      assert!(icons.raster_texture(&empty, true).is_none());
      assert!(!icons.has_icon(&empty));
   }

   /// `needs_fetch` is the read-only twin of `try_begin_fetch`, used to *count* the downloads a list
   /// load starts. It must report false for art we have, for a download in flight, and for a token
   /// that already missed this session — otherwise a capped loader spends its whole budget re-asking
   /// about the same rows and never reaches the tail of a long list.
   #[test]
   fn needs_fetch_mirrors_try_begin_fetch_without_claiming() {
      let icons = NftIcons::default();
      let key = (Address::from([0xbc; 20]), 1, U256::from(21));

      assert!(
         icons.needs_fetch(&key),
         "nothing known: a fetch is wanted"
      );
      assert!(
         icons.needs_fetch(&key),
         "probing must not claim it"
      );

      assert!(icons.try_begin_fetch(&key));
      assert!(
         !icons.needs_fetch(&key),
         "one is already in flight"
      );

      icons.finish_fetch(&key, true);
      assert!(!icons.needs_fetch(&key), "it missed this session");

      let with_art = (Address::from([0xbc; 20]), 1, U256::from(22));
      icons.insert_icon(
         with_art,
         raster(PLACEHOLDER.to_vec(), PLACEHOLDER.to_vec()),
      );
      assert!(
         !icons.needs_fetch(&with_art),
         "we already have it"
      );
   }
}
