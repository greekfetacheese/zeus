//! Shared helpers for the Railgun dapp flows.

pub mod merge_notes;
pub mod ops;
pub mod privacy;
pub mod shield;
pub mod transfer;
pub mod unshield;

pub use merge_notes::MergeNotesWindow;
pub use ops::*;
pub use privacy::{UnshieldPrivacy, assess_unshield};
pub use shield::{BundlerUrl, RailgunMode, ShieldUi};
pub use transfer::{private_merge_notes, private_transfer};
pub use unshield::default_bundler_url;

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::alloy_primitives::{Address, U256, address};
   use zeus_eth::currency::{Currency, ERC20Token};
   use zeus_eth::nft::{NftStandard, NftToken};
   use zeus_railgun::caip::AssetId;

   const BAYC: Address = address!("BC4CA0EdA7647A8aB7C2061c2E118A18a936f13D");

   fn nft(token_id: u64, standard: NftStandard) -> NftToken {
      NftToken {
         chain_id: 1,
         collection: BAYC,
         token_id: U256::from(token_id),
         standard,
         metadata_uri: None,
      }
   }

   /// A fungible asset is the currency it wraps, and the amount is whatever the user typed.
   #[test]
   fn a_fungible_asset_keeps_its_currency_and_amount() {
      let weth = Currency::from(ERC20Token::weth());
      let asset = RailgunAsset::Fungible(weth.clone());

      assert_eq!(
         asset.asset_id(),
         AssetId::Erc20(weth.to_erc20().address)
      );
      assert_eq!(asset.value(U256::from(1500)), U256::from(1500));
      assert!(!asset.is_native());
   }

   /// An ERC-721 is its collection and its id, and it is worth exactly one — there is no quantity to ask
   /// the user for, so whatever number reaches `value` cannot change what moves.
   #[test]
   fn an_erc721_asset_is_its_id_and_always_worth_one() {
      let asset = RailgunAsset::Nft(nft(7, NftStandard::Erc721));

      assert_eq!(
         asset.asset_id(),
         AssetId::Erc721(BAYC, U256::from(7))
      );
      assert_eq!(asset.value(U256::ZERO), U256::from(1));
      assert_eq!(
         asset.value(U256::MAX),
         U256::from(1),
         "the amount is not consulted"
      );
      assert!(!asset.is_native());
   }

   /// An ERC-1155 is the same collection and id under a **different asset type**, and it is a quantity of
   /// that id: the value is the amount the user asks for, counted in whole units, with nothing
   /// substituted for it.
   #[test]
   fn an_erc1155_asset_is_its_id_under_its_own_type_and_worth_its_amount() {
      let asset = RailgunAsset::Nft(nft(7, NftStandard::Erc1155));

      assert_eq!(
         asset.asset_id(),
         AssetId::Erc1155(BAYC, U256::from(7))
      );
      assert_eq!(asset.value(U256::from(1)), U256::from(1));
      assert_eq!(asset.value(U256::from(3)), U256::from(3));
      assert!(!asset.is_native());
   }

   /// Native ETH is the one asset that shields through `shield_native`, so the flag has to be right for
   /// it — and wrong for everything else.
   #[test]
   fn only_native_currency_is_native() {
      assert!(RailgunAsset::Fungible(Currency::native(1)).is_native());
      assert!(!RailgunAsset::Fungible(Currency::from(ERC20Token::weth())).is_native());
   }

   /// A native-ETH shield wraps inside the Railgun contract and an `unwrap_to_eth` unshield unwraps
   /// there, so the wallet's public wrapped-native balance is identical before and after: the refresh
   /// must not spend its retry budget on it. This is the defect the mapping exists to prevent.
   #[test]
   fn a_native_asset_waits_on_no_public_token() {
      assert!(matches!(
         RailgunAsset::Fungible(Currency::native(1)).settled_op(false),
         SettledOp::Fungible(None)
      ));
      assert!(matches!(
         RailgunAsset::Fungible(Currency::native(1)).settled_op(true),
         SettledOp::Fungible(None)
      ));
   }

   /// A public ERC-20 balance really does move, so its refresh keeps the retry — except when the
   /// operation unwraps it, which leaves the wallet's public wrapped-native balance put.
   #[test]
   fn an_erc20_asset_waits_unless_it_is_unwrapped() {
      let weth = ERC20Token::weth();

      match RailgunAsset::Fungible(Currency::from(weth.clone())).settled_op(false) {
         SettledOp::Fungible(Some(token)) => assert_eq!(token, weth),
         _ => panic!("an ERC-20 shield should re-read its token"),
      }

      assert!(matches!(
         RailgunAsset::Fungible(Currency::from(weth)).settled_op(true),
         SettledOp::Fungible(None)
      ));
   }

   /// An NFT moves public *ownership*, so it keeps its wait in either case.
   #[test]
   fn an_nft_asset_waits_on_ownership() {
      assert!(matches!(
         RailgunAsset::Nft(nft(7, NftStandard::Erc721)).settled_op(false),
         SettledOp::Nft
      ));
   }
}
