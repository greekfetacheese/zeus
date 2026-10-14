//! Catalog of everything Zeus writes under `data/`.
//!
//! Filenames and directory names live here — not as parallel `&str` constants
//! in the modules that happen to read or write them. Adding a persisted thing
//! means adding a variant; exhaustive matches (export policy, tree allow-rules,
//! optional-export flags) fail to compile until the new item is classified.
//!
//! `logs/` sits next to `data/` and is not part of this catalog.

use crate::utils::restrict_dir_to_owner;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// Directory name for the portable data tree (cwd-relative).
pub const DATA_DIR_NAME: &str = "data";

/// ERC-7730 index files cached under [`PersistedTree::ClearSigning`].
pub const CLEAR_SIGNING_INDEX_EIP712: &str = "index.eip712.json";
pub const CLEAR_SIGNING_INDEX_CALLDATA: &str = "index.calldata.json";

/// Downloaded token-icon basenames under [`PersistedTree::TokenIcons`].
pub const TOKEN_ICON_X32: &str = "x32.png";
pub const TOKEN_ICON_X24: &str = "x24.png";

/// Downloaded NFT-image basenames under [`PersistedTree::NftIcons`].
///
/// Art is rasterised at ingest into two renderings of the same picture, so switching between the grid
/// and the detail view never goes back to the network. A token directory holds those renderings plus
/// [`NFT_ICON_SOURCE`], and a vector file left by an older version is deleted on write — nothing reads
/// it any more. [`NFT_IMAGE_SVG`] stays a *known* name only so an archive written by that version
/// still validates.
pub const NFT_ICON_X64: &str = "x64.png";
pub const NFT_ICON_X250: &str = "x250.png";
pub const NFT_IMAGE_SVG: &str = "image.svg";
/// The metadata URI a token's cached art was read from, beside its renderings.
///
/// Kept so a collection that changes its `tokenURI` (a reveal, an upgrade) can be noticed: the cache is
/// keyed by `(collection, token id)`, which cannot tell the difference on its own.
pub const NFT_ICON_SOURCE: &str = "source_uri";

macro_rules! persisted_files {
   ($($variant:ident => $name:literal),* $(,)?) => {
      /// A single known file at the root of `data/`.
      #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
      pub enum PersistedFile {
         $($variant,)*
      }

      impl PersistedFile {
         pub const ALL: &[Self] = &[$(Self::$variant,)*];

         pub const fn name(self) -> &'static str {
            match self {
               $(Self::$variant => $name,)*
            }
         }

         pub fn from_name(name: &str) -> Option<Self> {
            match name {
               $($name => Some(Self::$variant),)*
               _ => None,
            }
         }
      }
   };
}

macro_rules! persisted_trees {
   ($($variant:ident => $name:literal),* $(,)?) => {
      /// A known directory tree under `data/` with generated child names.
      #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
      pub enum PersistedTree {
         $($variant,)*
      }

      impl PersistedTree {
         pub const ALL: &[Self] = &[$(Self::$variant,)*];

         pub const fn dir_name(self) -> &'static str {
            match self {
               $(Self::$variant => $name,)*
            }
         }

         pub fn from_dir_name(name: &str) -> Option<Self> {
            match name {
               $($name => Some(Self::$variant),)*
               _ => None,
            }
         }
      }
   };
}

persisted_files! {
   Vault => "vault.data",
   WalletState => "wallet_state.data",
   TxHistory => "tx_history.db",
   Tokens => "tokens.data",
   NftDb => "nft_db.data",
   PoolData => "pool_data.data",
   Providers => "providers.data",
   BundlerUrl => "bundler_url.data",
   AddressBook => "address_book.data",
   PriceData => "price_data.data",
   Theme => "theme.json",
   ServerPort => "server_port.json",
   DisabledChains => "disabled_chains.json",
   RailgunConfig => "railgun_config.json",
   MiscConfig => "misc_config.json",
   Security => "security.data",
   AcrossSettings => "across_settings.json",
   ClientSettings => "client_settings.json",
   Connector => "connector.json",
   NativeHostManifest => "io.github.zeus_wallet.json",
   NativeHostWrapperUnix => "zeus_connector_host.sh",
   NativeHostWrapperWindows => "zeus_connector_host.cmd",
}

persisted_trees! {
   Railgun => "railgun",
   ClearSigning => "clear_signing",
   TokenIcons => "token_icons",
   NftIcons => "nft_icons",
}

/// Everything Zeus persists under `data/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Persisted {
   File(PersistedFile),
   Tree(PersistedTree),
}

/// How a catalog entry is treated by export / import.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportPolicy {
   /// Packed if present; allowed in an import zip.
   Core,
   /// Packed only when the matching [`ExportOptions`] flag is set.
   Optional,
   /// Written at runtime, never backed up (`connector.json`, native-host files).
   Never,
}

impl Persisted {
   pub const fn export_policy(self) -> ExportPolicy {
      match self {
         Self::File(
            PersistedFile::Vault
            | PersistedFile::WalletState
            | PersistedFile::TxHistory
            | PersistedFile::Tokens
            | PersistedFile::NftDb
            | PersistedFile::PoolData
            | PersistedFile::Providers
            | PersistedFile::BundlerUrl
            | PersistedFile::AddressBook
            | PersistedFile::PriceData
            | PersistedFile::Theme
            | PersistedFile::ServerPort
            | PersistedFile::DisabledChains
            | PersistedFile::RailgunConfig
            | PersistedFile::MiscConfig
            | PersistedFile::Security
            | PersistedFile::AcrossSettings
            | PersistedFile::ClientSettings,
         ) => ExportPolicy::Core,
         Self::File(
            PersistedFile::Connector
            | PersistedFile::NativeHostManifest
            | PersistedFile::NativeHostWrapperUnix
            | PersistedFile::NativeHostWrapperWindows,
         ) => ExportPolicy::Never,
         Self::Tree(_) => ExportPolicy::Optional,
      }
   }
}

impl PersistedFile {
   pub const fn export_policy(self) -> ExportPolicy {
      Persisted::File(self).export_policy()
   }

   /// Whether the file is sealed with the vault-held [`super::WalletStateKey`].
   ///
   /// A sealed file opens only with the key in [`PersistedFile::Vault`] — so replacing that vault
   /// (an import) makes every sealed store the archive does not carry unreadable. That is what an
   /// import has to account for; see `data_import`.
   ///
   /// [`PersistedFile::Vault`] is *not* listed: it is sealed with the user's credentials rather than
   /// the key, and an import always carries it.
   pub const fn is_sealed(self) -> bool {
      match self {
         Self::WalletState
         | Self::TxHistory
         | Self::Tokens
         | Self::NftDb
         | Self::PoolData
         | Self::Providers
         | Self::BundlerUrl
         | Self::AddressBook
         | Self::PriceData
         | Self::Security => true,
         Self::Vault
         | Self::Theme
         | Self::ServerPort
         | Self::DisabledChains
         | Self::RailgunConfig
         | Self::MiscConfig
         | Self::AcrossSettings
         | Self::ClientSettings
         | Self::Connector
         | Self::NativeHostManifest
         | Self::NativeHostWrapperUnix
         | Self::NativeHostWrapperWindows => false,
      }
   }
}

impl PersistedTree {
   pub const fn export_policy(self) -> ExportPolicy {
      Persisted::Tree(self).export_policy()
   }

   /// Checkbox copy in the export UI. Required for every tree so a new optional
   /// tree cannot ship without a label.
   pub const fn export_label(self) -> &'static str {
      match self {
         Self::Railgun => "Include Railgun data",
         Self::ClearSigning => "Include clear signing cache",
         Self::TokenIcons => "Include downloaded token icons",
         Self::NftIcons => "Include downloaded NFT images",
      }
   }

   /// Whether `rel` (relative to this tree's directory) is a known child path.
   pub fn allows_rel(self, rel: &Path) -> bool {
      match self {
         Self::Railgun => {
            let Some(name) = single_normal_name(rel) else {
               return false;
            };
            is_allowed_railgun_file(name)
         }
         Self::ClearSigning => {
            let Some(name) = single_normal_name(rel) else {
               return false;
            };
            is_allowed_clear_signing_file(name)
         }
         Self::TokenIcons => is_allowed_token_icon_rel(rel),
         Self::NftIcons => is_allowed_nft_icon_rel(rel),
      }
   }
}

/// Optional trees the user can add on top of the core files.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExportOptions {
   pub clear_signing: bool,
   pub railgun: bool,
   pub token_icons: bool,
   pub nft_icons: bool,
}

impl ExportOptions {
   pub fn include_tree(self, tree: PersistedTree) -> bool {
      match tree {
         PersistedTree::Railgun => self.railgun,
         PersistedTree::ClearSigning => self.clear_signing,
         PersistedTree::TokenIcons => self.token_icons,
         PersistedTree::NftIcons => self.nft_icons,
      }
   }

   pub fn flag_mut(&mut self, tree: PersistedTree) -> &mut bool {
      match tree {
         PersistedTree::Railgun => &mut self.railgun,
         PersistedTree::ClearSigning => &mut self.clear_signing,
         PersistedTree::TokenIcons => &mut self.token_icons,
         PersistedTree::NftIcons => &mut self.nft_icons,
      }
   }
}

/// Zeus `data/` directory (cwd-relative). Creates it if missing.
pub fn data_dir() -> Result<PathBuf, anyhow::Error> {
   let dir = std::env::current_dir()?.join(DATA_DIR_NAME);

   if !dir.exists() {
      std::fs::create_dir_all(dir.clone())?;
   }

   if let Err(e) = restrict_dir_to_owner(&dir) {
      tracing::warn!("Failed to restrict data/ permissions: {e}");
   }

   Ok(dir)
}

pub fn file_path(file: PersistedFile) -> Result<PathBuf, anyhow::Error> {
   Ok(data_dir()?.join(file.name()))
}

/// Suffix a store is renamed to when it cannot be read. See [`UnreadableStores`].
const QUARANTINE_MARKER: &str = "quarantine";

/// A store whose unreadable bytes are still at their path, so a save would destroy them.
#[derive(Debug, Clone)]
pub struct BlockedStore {
   /// Where the unreadable bytes still sit.
   pub path: PathBuf,
   /// The load error, kept so a saver can say *why* it is skipping.
   pub reason: String,
}

/// The stores whose unreadable bytes are still at their path this run.
///
/// A failed load leaves the in-memory store at its `Default`, so a save would seal that default
/// over the user's real bytes. A loader hands a failed read to [`quarantine_unreadable_store`],
/// which renames the file aside; only when that rename *fails* are the bytes still at the path, and
/// only then is the store recorded here. Every saver consults [`Self::save_blocked`] and skips
/// while an entry is present.
///
/// So the record means exactly: **a save would still destroy the bytes at this path.** Once the
/// bytes are moved aside (or were never there), the store is not recorded and its saver may write
/// again — which is how a deliberate format break is adopted in the same run instead of the next
/// start. A record is not cleared by a later load; it lasts for the rest of the run.
#[derive(Debug, Default, Clone)]
pub struct UnreadableStores {
   stores: HashMap<PersistedFile, BlockedStore>,
}

impl UnreadableStores {
   /// The reason `file` must not be saved, or `None` when saving it is allowed.
   ///
   /// `Some` means the store's unreadable bytes are still at their path: a saver that gets it must
   /// skip its write and log the reason. `None` means there is nothing left to protect, so the
   /// saver may write — adopting the new format, when the failure was a deliberate format break.
   pub fn save_blocked(&self, file: PersistedFile) -> Option<String> {
      self.stores.get(&file).map(|store| {
         format!(
            "{} could not be read and its unreadable bytes are still at {}: {}",
            file.name(),
            store.path.display(),
            store.reason
         )
      })
   }

   fn record(&mut self, file: PersistedFile, path: PathBuf, reason: String) {
      self.stores.insert(file, BlockedStore { path, reason });
   }
}

/// Move an unreadable store aside, or record it when it cannot be moved.
///
/// The file is renamed so its bytes survive and the next start is not fighting a file that cannot
/// be read. After a successful rename there is nothing at the path left to protect, so the store is
/// **not** recorded and its savers may write again — the adoption a deliberate format break needs.
/// Only when the rename fails are the unreadable bytes still at the path, and the store is recorded
/// so no saver destroys them — see [`UnreadableStores::save_blocked`]. `None` means nothing is
/// blocked: the file was already gone (a first run) or it was moved aside.
pub fn quarantine_unreadable_store(
   stores: &mut UnreadableStores,
   file: PersistedFile,
   reason: &anyhow::Error,
) -> Option<PathBuf> {
   let path = file_path(file).ok()?;
   quarantine_unreadable_at(stores, file, &path, reason)
}

/// The path-taking half of [`quarantine_unreadable_store`], so its behaviour can be pinned by a
/// test without moving the process working directory.
fn quarantine_unreadable_at(
   stores: &mut UnreadableStores,
   file: PersistedFile,
   path: &Path,
   reason: &anyhow::Error,
) -> Option<PathBuf> {
   if !path.exists() {
      return None;
   }

   match quarantine_file_at(path) {
      Ok(moved) => {
         // The bytes are no longer at `path`, so there is nothing left to protect: do not record.
         // A store that failed on content (an old format, a corrupt blob, a torn write) may now
         // write its default, which is how a format break is adopted in the same run.
         tracing::error!(
            "{} could not be read; moved aside to {}: {reason:?}",
            path.display(),
            moved.display()
         );
         Some(moved)
      }
      Err(e) => {
         // The bytes could not be moved, so they are still at the path. Record the store: a saver
         // that wrote defaults over a file that is right there is worse than a skipped save.
         tracing::error!(
            "{} could not be read and could not be moved aside ({e}); it will not be overwritten",
            path.display()
         );
         stores.record(
            file,
            path.to_path_buf(),
            format!("{reason:?} (quarantine failed: {e})"),
         );
         None
      }
   }
}

/// Rename `path` to a free quarantine name beside it and return the new path.
///
/// The name is `<name>.quarantine-<unix-millis>`, with `-<n>` appended until it is free. The
/// timestamp separates quarantines from different starts; the `-<n>` tail means even two in the
/// same millisecond cannot land on the same name, so a later failed start never clobbers an
/// earlier quarantine. The name is not a known [`PersistedFile`], so an import zip cannot carry
/// one.
fn quarantine_file_at(path: &Path) -> std::io::Result<PathBuf> {
   let file_name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
      std::io::Error::new(
         std::io::ErrorKind::InvalidInput,
         "path has no file name",
      )
   })?;

   let stamp = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|elapsed| elapsed.as_millis())
      .unwrap_or_default();

   for attempt in 0u32.. {
      let candidate = if attempt == 0 {
         path.with_file_name(format!("{file_name}.{QUARANTINE_MARKER}-{stamp}"))
      } else {
         path.with_file_name(format!(
            "{file_name}.{QUARANTINE_MARKER}-{stamp}-{attempt}"
         ))
      };
      if !candidate.exists() {
         std::fs::rename(path, &candidate)?;
         return Ok(candidate);
      }
   }

   unreachable!("a free quarantine name always exists")
}

/// Tree directory under `data/`. Creates it if missing.
pub fn tree_dir(tree: PersistedTree) -> Result<PathBuf, anyhow::Error> {
   let dir = data_dir()?.join(tree.dir_name());
   if !dir.exists() {
      std::fs::create_dir_all(&dir)?;
   }
   match tree {
      PersistedTree::Railgun => {
         if let Err(e) = restrict_dir_to_owner(&dir) {
            tracing::warn!(
               "Failed to restrict data/{}/ permissions: {e}",
               tree.dir_name()
            );
         }
      }
      PersistedTree::ClearSigning | PersistedTree::TokenIcons | PersistedTree::NftIcons => {}
   }
   Ok(dir)
}

pub fn railgun_dir() -> Result<PathBuf, anyhow::Error> {
   tree_dir(PersistedTree::Railgun)
}

pub fn railgun_db_file(chain: u64) -> Result<PathBuf, anyhow::Error> {
   Ok(railgun_dir()?.join(format!("railgun:{}.db", chain)))
}

pub fn theme_kind_dir() -> Result<PathBuf, anyhow::Error> {
   file_path(PersistedFile::Theme)
}

pub fn server_port_dir() -> Result<PathBuf, anyhow::Error> {
   file_path(PersistedFile::ServerPort)
}

pub fn disabled_chains_dir() -> Result<PathBuf, anyhow::Error> {
   file_path(PersistedFile::DisabledChains)
}

pub fn railgun_config_dir() -> Result<PathBuf, anyhow::Error> {
   file_path(PersistedFile::RailgunConfig)
}

pub fn misc_config_dir() -> Result<PathBuf, anyhow::Error> {
   file_path(PersistedFile::MiscConfig)
}

pub fn security_dir() -> Result<PathBuf, anyhow::Error> {
   file_path(PersistedFile::Security)
}

pub fn client_settings_dir() -> Result<PathBuf, anyhow::Error> {
   file_path(PersistedFile::ClientSettings)
}

pub fn pool_data_dir() -> Result<PathBuf, anyhow::Error> {
   file_path(PersistedFile::PoolData)
}

pub fn bundler_url_dir() -> Result<PathBuf, anyhow::Error> {
   file_path(PersistedFile::BundlerUrl)
}

pub fn is_allowed_clear_signing_file(name: &str) -> bool {
   name == CLEAR_SIGNING_INDEX_EIP712
      || name == CLEAR_SIGNING_INDEX_CALLDATA
      || is_keccak256_json(name)
}

pub fn is_allowed_railgun_file(name: &str) -> bool {
   if let Some(rest) = name.strip_prefix("railgun:") {
      return rest.strip_suffix(".db").is_some_and(is_chain_id);
   }
   if let Some(rest) = name.strip_prefix("events-snapshot:") {
      return rest.strip_suffix(".db").is_some_and(is_chain_id);
   }
   false
}

pub fn is_allowed_token_icon_rel(rel: &Path) -> bool {
   let Some(parts) = normal_components(rel) else {
      return false;
   };
   if parts.len() != 3 {
      return false;
   }
   is_chain_id(&parts[0]) && is_token_address_dir(&parts[1]) && is_icon_file(&parts[2])
}

/// NFT images live at `nft_icons/{chain}/{collection}/{tokenId}/{x64,x250}.png`, beside the
/// `source_uri` sidecar that records the metadata URI they were read from.
///
/// One directory per token id because the id is a `uint256`, not a fixed-size value. The id is
/// stored in **decimal** and leading zeros are rejected, so `1` and `01` cannot become two
/// directories for the same token.
pub fn is_allowed_nft_icon_rel(rel: &Path) -> bool {
   let Some(parts) = normal_components(rel) else {
      return false;
   };
   if parts.len() != 4 {
      return false;
   }
   is_chain_id(&parts[0])
      && is_token_address_dir(&parts[1])
      && is_nft_token_id_dir(&parts[2])
      && is_nft_icon_file(&parts[3])
}

/// Whether `parts` (relative to `data/`) is a known persisted path that may
/// appear in an export zip.
pub fn is_allowed_rel_parts(parts: &[String]) -> bool {
   match parts {
      [name] => {
         PersistedFile::from_name(name).is_some_and(|f| f.export_policy() != ExportPolicy::Never)
      }
      [dir, rest @ ..] => {
         let Some(tree) = PersistedTree::from_dir_name(dir) else {
            return false;
         };
         if tree.export_policy() == ExportPolicy::Never {
            return false;
         }
         let rel: PathBuf = rest.iter().collect();
         tree.allows_rel(&rel)
      }
      _ => false,
   }
}

pub(crate) fn normal_components(path: &Path) -> Option<Vec<String>> {
   let mut parts = Vec::new();
   for component in path.components() {
      match component {
         Component::Normal(s) => {
            let s = s.to_str()?;
            if s.is_empty() || s == "." || s == ".." {
               return None;
            }
            parts.push(s.to_string());
         }
         Component::CurDir => {}
         _ => return None,
      }
   }
   Some(parts)
}

fn single_normal_name(rel: &Path) -> Option<&str> {
   let mut comps = rel.components();
   let Component::Normal(name) = comps.next()? else {
      return None;
   };
   if comps.next().is_some() {
      return None;
   }
   let name = name.to_str()?;
   if name.is_empty() || name == "." || name == ".." {
      return None;
   }
   Some(name)
}

fn is_chain_id(s: &str) -> bool {
   !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

fn is_keccak256_json(name: &str) -> bool {
   let Some(stem) = name.strip_suffix(".json") else {
      return false;
   };
   stem.len() == 64 && stem.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_token_address_dir(s: &str) -> bool {
   let Some(hex) = s.strip_prefix("0x") else {
      return false;
   };
   hex.len() == 40 && hex.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_icon_file(name: &str) -> bool {
   name == TOKEN_ICON_X32
}

/// Token-id directory names: decimal, no leading zeros, at most 78 digits (a `uint256` is at most
/// 78 characters) so a hostile import zip cannot spell an absurd path.
fn is_nft_token_id_dir(s: &str) -> bool {
   if s.is_empty() || s.len() > 78 || !s.chars().all(|c| c.is_ascii_digit()) {
      return false;
   }
   s == "0" || !s.starts_with('0')
}

fn is_nft_icon_file(name: &str) -> bool {
   name == NFT_ICON_X64 || name == NFT_ICON_X250 || name == NFT_IMAGE_SVG || name == NFT_ICON_SOURCE
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn file_names_are_unique() {
      let mut names: Vec<_> = PersistedFile::ALL.iter().map(|f| f.name()).collect();
      names.sort_unstable();
      names.dedup();
      assert_eq!(names.len(), PersistedFile::ALL.len());
   }

   #[test]
   fn tree_dir_names_are_unique() {
      let mut names: Vec<_> = PersistedTree::ALL.iter().map(|t| t.dir_name()).collect();
      names.sort_unstable();
      names.dedup();
      assert_eq!(names.len(), PersistedTree::ALL.len());
   }

   #[test]
   fn core_files_include_vault_and_wallet_state() {
      let names: Vec<_> = PersistedFile::ALL
         .iter()
         .copied()
         .filter(|f| f.export_policy() == ExportPolicy::Core)
         .map(PersistedFile::name)
         .collect();
      assert!(names.contains(&"vault.data"));
      assert!(names.contains(&"wallet_state.data"));
      assert!(names.contains(&"misc_config.json"));
      assert!(!names.contains(&"connector.json"));
   }

   /// The NFT catalog is user data: it must travel with an export exactly as `tokens.data` does.
   /// A newly added store that nobody classified would silently drop out of every backup.
   #[test]
   fn nft_db_is_a_core_exported_file() {
      assert_eq!(PersistedFile::NftDb.name(), "nft_db.data");
      assert_eq!(
         PersistedFile::NftDb.export_policy(),
         ExportPolicy::Core
      );
      assert!(
         is_allowed_rel_parts(&["nft_db.data".to_string()]),
         "an export zip must accept nft_db.data"
      );
      assert!(
         !is_allowed_rel_parts(&["nft_db.data.bak".to_string()]),
         "only the exact file name is a known path"
      );
   }

   /// Security settings are user data: they must travel with an export exactly as
   /// `wallet_state.data` does, and the sealed file must be a known path.
   #[test]
   fn security_is_a_core_exported_file() {
      assert_eq!(PersistedFile::Security.name(), "security.data");
      assert_eq!(
         PersistedFile::Security.export_policy(),
         ExportPolicy::Core
      );
      assert!(
         is_allowed_rel_parts(&["security.data".to_string()]),
         "an export zip must accept security.data"
      );
      assert!(
         !is_allowed_rel_parts(&["security.data.bak".to_string()]),
         "only the exact file name is a known path"
      );
   }

   /// NFT image paths are `nft_icons/{chain}/{collection}/{tokenId}/x*.png` and nothing else.
   #[test]
   fn nft_icon_paths_are_strictly_validated() {
      let good = |rel: &str| is_allowed_nft_icon_rel(&PathBuf::from(rel));
      let bayc = "0xbc4ca0eda7647a8ab7c2061c2e118a18a936f13d";

      assert!(good(&format!("1/{bayc}/1/x64.png")));
      assert!(good(&format!("1/{bayc}/1/x250.png")));
      assert!(
         good(&format!("1/{bayc}/1/image.svg")),
         "a vector file from an older version is still a known path"
      );
      assert!(
         good(&format!("1/{bayc}/1/{NFT_ICON_SOURCE}")),
         "the metadata-URI sidecar is part of the token directory"
      );
      assert!(
         good(&format!("137/{bayc}/0/x64.png")),
         "token id 0 is valid"
      );
      assert!(
         good(&format!("1/{bayc}/{}/x64.png", u128::MAX)),
         "a large token id is valid"
      );

      assert!(
         !good(&format!("1/{bayc}/01/x64.png")),
         "a leading zero would make two directories for one token"
      );
      assert!(
         !good(&format!("1/{bayc}/x64.png")),
         "the token id directory is required"
      );
      assert!(
         !good(&format!("1/{bayc}/1/extra/x64.png")),
         "no extra nesting"
      );
      assert!(!good(&format!("1/not-an-address/1/x64.png")));
      assert!(
         !good(&format!("1/{bayc}/1/logo-32.png")),
         "only our own file names"
      );
      assert!(
         !good(&format!("1/{bayc}/1/../1/x64.png")),
         "traversal is not a known path"
      );

      // The two icon trees must not accept each other's layout.
      assert!(!good(&format!("1/{bayc}/x32.png")));
      assert!(is_allowed_token_icon_rel(&PathBuf::from(
         format!("1/{bayc}/x32.png")
      )));
      assert!(!is_allowed_token_icon_rel(&PathBuf::from(
         format!("1/{bayc}/1/x64.png")
      )));

      // And the tree dispatch finds it through `is_allowed_rel_parts`.
      assert!(is_allowed_rel_parts(&[
         "nft_icons".to_string(),
         "1".to_string(),
         bayc.to_string(),
         "1".to_string(),
         "x64.png".to_string(),
      ]));
   }

   /// A new tree must be Optional with a label, or the export dialog would either skip it or render
   /// a checkbox with no text.
   #[test]
   fn nft_icons_tree_is_exportable_with_a_label() {
      assert_eq!(PersistedTree::NftIcons.dir_name(), "nft_icons");
      assert_eq!(
         PersistedTree::NftIcons.export_policy(),
         ExportPolicy::Optional
      );
      assert!(!PersistedTree::NftIcons.export_label().is_empty());
      assert!(
         !ExportOptions::default().nft_icons,
         "images stay opt-in for an export too"
      );
   }

   #[test]
   fn machine_local_files_are_never_exported() {
      for file in [
         PersistedFile::Connector,
         PersistedFile::NativeHostManifest,
         PersistedFile::NativeHostWrapperUnix,
         PersistedFile::NativeHostWrapperWindows,
      ] {
         assert_eq!(file.export_policy(), ExportPolicy::Never);
         assert!(!is_allowed_rel_parts(&[file.name().to_string()]));
      }
   }

   #[test]
   fn from_name_roundtrips() {
      for file in PersistedFile::ALL {
         assert_eq!(PersistedFile::from_name(file.name()), Some(*file));
      }
      assert!(PersistedFile::from_name("secret.txt").is_none());
   }

   /// A scratch directory for the quarantine tests, unique per test and process so parallel test
   /// threads cannot collide.
   fn scratch_dir(tag: &str) -> PathBuf {
      let dir = std::env::temp_dir().join(format!(
         "zeus-persisted-{tag}-{}-{}",
         std::process::id(),
         std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default()
      ));
      std::fs::create_dir_all(&dir).unwrap();
      dir
   }

   /// A file of garbage bytes at a store's path is an unreadable store. When the bytes can be moved
   /// aside they are protected by the rename, and the store is *not* blocked: a save proceeds and
   /// adopts the new format in the same run.
   #[test]
   fn a_moved_aside_store_is_no_longer_blocked() {
      let dir = scratch_dir("moved");
      let path = dir.join(PersistedFile::Tokens.name());
      std::fs::write(&path, b"not a sealed blob").unwrap();

      let mut stores = UnreadableStores::default();
      let err = anyhow::anyhow!("aead: aad mismatch");
      let moved = quarantine_unreadable_at(&mut stores, PersistedFile::Tokens, &path, &err)
         .expect("the bytes are moved aside");

      assert!(
         !path.exists(),
         "the original name no longer holds the garbage"
      );
      assert_eq!(
         std::fs::read(&moved).unwrap(),
         b"not a sealed blob",
         "the quarantined file holds the garbage"
      );
      assert!(
         stores.save_blocked(PersistedFile::Tokens).is_none(),
         "after the rename there is nothing left to protect, so the store may save"
      );

      // What every saver does: write when the record allows it. Nothing blocks, so the store is
      // written afresh — the adoption a deliberate format break needs — and the quarantined bytes
      // survive.
      if stores.save_blocked(PersistedFile::Tokens).is_none() {
         std::fs::write(&path, b"fresh format").unwrap();
      }
      assert_eq!(
         std::fs::read(&path).unwrap(),
         b"fresh format",
         "the store is written again, adopting the new format in the same run"
      );
      assert_eq!(
         std::fs::read(&moved).unwrap(),
         b"not a sealed blob"
      );

      std::fs::remove_dir_all(&dir).ok();
   }

   /// When the bytes cannot be moved they are still at the path, so the store is recorded and a save
   /// is blocked, with a reason naming where they sit.
   #[cfg(unix)]
   #[test]
   fn a_store_whose_bytes_cannot_be_moved_is_recorded_and_blocked() {
      use std::os::unix::fs::PermissionsExt;

      let dir = scratch_dir("readonly");
      let path = dir.join(PersistedFile::Tokens.name());
      std::fs::write(&path, b"not a sealed blob").unwrap();

      // A read-only parent makes `rename` fail: moving an entry needs write permission on its
      // directory. (This only holds for a non-root user; root bypasses directory permissions.)
      std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();

      let mut stores = UnreadableStores::default();
      let err = anyhow::anyhow!("aead: aad mismatch");
      let moved = quarantine_unreadable_at(&mut stores, PersistedFile::Tokens, &path, &err);

      // Restore the directory before asserting, so a failed assertion cannot leave it undeletable.
      std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

      assert!(
         moved.is_none(),
         "a rename that failed reports no quarantine path"
      );
      assert!(
         path.exists(),
         "the unreadable bytes are still at the path"
      );
      let reason = stores
         .save_blocked(PersistedFile::Tokens)
         .expect("the store is recorded while its bytes cannot be moved");
      assert!(
         reason.contains(&path.display().to_string()),
         "the reason names where the bytes still are: {reason}"
      );

      std::fs::remove_dir_all(&dir).ok();
   }

   /// A second failed start must not clobber the first quarantine: the `-<n>` tail finds a free
   /// name even when the timestamp is identical.
   #[test]
   fn a_second_quarantine_does_not_collide() {
      let dir = scratch_dir("collision");
      let path = dir.join(PersistedFile::PriceData.name());

      std::fs::write(&path, b"first").unwrap();
      let first = quarantine_file_at(&path).unwrap();
      std::fs::write(&path, b"second").unwrap();
      let second = quarantine_file_at(&path).unwrap();

      assert_ne!(first, second);
      assert_eq!(std::fs::read(&first).unwrap(), b"first");
      assert_eq!(std::fs::read(&second).unwrap(), b"second");

      std::fs::remove_dir_all(&dir).ok();
   }

   /// A store that was never on disk is a first run, not a failure: nothing is moved and nothing is
   /// blocked, so its default may be written.
   #[test]
   fn a_missing_store_is_not_quarantined() {
      let dir = scratch_dir("missing");
      let path = dir.join(PersistedFile::NftDb.name());

      let mut stores = UnreadableStores::default();
      let err = anyhow::anyhow!("no file");
      assert!(quarantine_unreadable_at(&mut stores, PersistedFile::NftDb, &path, &err).is_none());
      assert!(stores.save_blocked(PersistedFile::NftDb).is_none());

      std::fs::remove_dir_all(&dir).ok();
   }
}
