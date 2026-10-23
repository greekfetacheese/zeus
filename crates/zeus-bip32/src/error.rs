use thiserror::Error;

#[derive(Debug, Error)]
pub enum Bip32Error {
   /// Error bubbled up from the backend
   #[error("k256 error")]
   BackendError(/*#[from]*/ k256::ecdsa::Error),

   /// Error bubbled up from the backend
   #[error("elliptic curve error")]
   EllipticCurveError(/*#[from]*/ k256::elliptic_curve::Error),

   /// Error bubbled up from std::io
   #[error(transparent)]
   IoError(#[from] std::io::Error),

   /// Master key seed generation received <16 bytes
   #[error("Master key seed generation received <16 bytes")]
   SeedTooShort,

   /// HMAC I_L was invalid during key generation.
   #[error("HMAC left segment was 0 or greater than the curve order. How?")]
   InvalidKey,

   /// Attempted to derive the hardened child of an xpub
   #[error("Attempted to derive the hardened child of an xpub")]
   HardenedDerivationFailed,

   /// Attempted to tweak an xpriv or xpub directly
   #[error("Attempted to tweak an xpriv or xpub directly")]
   BadTweak,

   /// Child derivation would overflow the serialized depth byte
   #[error("Derivation depth exceeds 255")]
   DepthOverflow,

   /// Unrecognized version when deserializing xpriv
   #[error("Version bytes 0x{0:x?} don't match any network xpriv version bytes")]
   BadXPrivVersionBytes([u8; 4]),

   /// Unrecognized version when deserializing xpub
   #[error("Version bytes 0x{0:x?} don't match any network xpub version bytes")]
   BadXPubVersionBytes([u8; 4]),

   /// Bad padding byte on serialized xprv
   #[error("Expected 0 padding byte. Got {0}")]
   BadPadding(u8),

   /// Bad Checks on b58check
   #[error("Checksum mismatch on b58 deserialization")]
   BadB58Checksum,

   /// Parsing an string derivation failed because an index string was malformatted
   #[error("Malformatted index during derivation: {0}")]
   MalformattedDerivation(String),

   /// Attempted to deserialize a DER signature to a recoverable signature.
   #[error(
      "Attempted to deserialize a DER signature to a recoverable signature. Use deserialize_vrs instead"
   )]
   NoRecoveryId,

   /// Attempted to deserialize a very long path
   #[error("Invalid Bip32 Path.")]
   InvalidBip32Path,

   #[error("{0}")]
   Custom(String),
}

impl From<k256::ecdsa::Error> for Bip32Error {
   fn from(e: k256::ecdsa::Error) -> Self {
      Self::BackendError(e)
   }
}

impl From<k256::elliptic_curve::Error> for Bip32Error {
   fn from(e: k256::elliptic_curve::Error) -> Self {
      Self::EllipticCurveError(e)
   }
}

impl From<std::convert::Infallible> for Bip32Error {
   fn from(_i: std::convert::Infallible) -> Self {
      unimplemented!("unreachable, but required by type system")
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn pins_rendered_messages_and_sources() {
      assert_eq!(
         Bip32Error::SeedTooShort.to_string(),
         "Master key seed generation received <16 bytes"
      );
      assert_eq!(
         Bip32Error::InvalidKey.to_string(),
         "HMAC left segment was 0 or greater than the curve order. How?"
      );
      assert_eq!(
         Bip32Error::HardenedDerivationFailed.to_string(),
         "Attempted to derive the hardened child of an xpub"
      );
      assert_eq!(
         Bip32Error::BadTweak.to_string(),
         "Attempted to tweak an xpriv or xpub directly"
      );
      assert_eq!(
         Bip32Error::DepthOverflow.to_string(),
         "Derivation depth exceeds 255"
      );
      assert_eq!(
         Bip32Error::BadXPrivVersionBytes([0x04, 0x88, 0xb2, 0x1e]).to_string(),
         "Version bytes 0x[4, 88, b2, 1e] don't match any network xpriv version bytes"
      );
      assert_eq!(
         Bip32Error::BadXPubVersionBytes([0x04, 0x88, 0xb2, 0x1e]).to_string(),
         "Version bytes 0x[4, 88, b2, 1e] don't match any network xpub version bytes"
      );
      assert_eq!(
         Bip32Error::BadPadding(7).to_string(),
         "Expected 0 padding byte. Got 7"
      );
      assert_eq!(
         Bip32Error::BadB58Checksum.to_string(),
         "Checksum mismatch on b58 deserialization"
      );
      assert_eq!(
         Bip32Error::MalformattedDerivation("x".to_string()).to_string(),
         "Malformatted index during derivation: x"
      );
      assert_eq!(
         Bip32Error::NoRecoveryId.to_string(),
         "Attempted to deserialize a DER signature to a recoverable signature. Use \
          deserialize_vrs instead"
      );
      assert_eq!(
         Bip32Error::InvalidBip32Path.to_string(),
         "Invalid Bip32 Path."
      );
      assert_eq!(
         Bip32Error::Custom("boom".to_string()).to_string(),
         "boom"
      );

      // `#[error(transparent)]` hands `source()` the inner error's own source, and a plain
      // `std::io::Error` carries none. Pin the measured delegation, not a wrapping `Some`.
      let io = Bip32Error::IoError(std::io::Error::new(
         std::io::ErrorKind::Other,
         "boom",
      ));
      assert_eq!(io.to_string(), "boom");
      assert!(std::error::Error::source(&io).is_none());

      // The delegation is observable once the wrapped error carries a source of its own.
      #[derive(Debug)]
      struct Root;

      impl std::fmt::Display for Root {
         fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("root")
         }
      }

      impl std::error::Error for Root {}

      #[derive(Debug)]
      struct WithSource(Root);

      impl std::fmt::Display for WithSource {
         fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("outer")
         }
      }

      impl std::error::Error for WithSource {
         fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
         }
      }

      let nested = Bip32Error::IoError(std::io::Error::new(
         std::io::ErrorKind::Other,
         WithSource(Root),
      ));
      assert_eq!(nested.to_string(), "outer");
      let source = std::error::Error::source(&nested).expect("transparent forwards the source");
      assert_eq!(source.to_string(), "root");

      assert!(std::error::Error::source(&Bip32Error::SeedTooShort).is_none());
      assert!(matches!(
         Bip32Error::from(std::io::Error::new(
            std::io::ErrorKind::Other,
            "boom"
         )),
         Bip32Error::IoError(_)
      ));

      assert_eq!(
         Bip32Error::BackendError(k256::ecdsa::Error::new()).to_string(),
         "k256 error"
      );
      assert_eq!(
         Bip32Error::EllipticCurveError(k256::elliptic_curve::Error).to_string(),
         "elliptic curve error"
      );
   }
}
