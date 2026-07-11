# This is a fork from [ethereum/kohaku](https://github.com/ethereum/kohaku/tree/master/crates/railgun) with some modifications to make it work with Zeus.

# Major differences

- Any kind of secret/private key is handled by the `secure-types` crate, zeroize is also used where needed.
- Added support for decoding legacy events, that means that the RpcSyncer can be used to fully sync the indexer from scratch
without the need of the SubsquidSyncer.
- Added a redb database implementation for disk persistence.
- Added disk persistence for the RemoteArtifactLoader.
- Tree state serialization with bincode which is much more space efficient than JSON.

# This crate is still in development and is not ready for production use.