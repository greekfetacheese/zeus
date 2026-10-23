/// The error type returned by every fallible operation in this crate.
#[derive(Debug)]
#[non_exhaustive]
pub enum Bip32Error {
   /// Error bubbled up from the backend
   BackendError(k256::ecdsa::Error),

   /// A scalar or point fell outside the elliptic curve group
   EllipticCurveError(k256::elliptic_curve::Error),

   /// Error bubbled up from std::io
   IoError(std::io::Error),

   /// A `secure_types` operation failed (secure allocation, locking or UTF-8).
   SecureError(secure_types::Error),

   /// Master key seed generation received <16 bytes
   SeedTooShort,

   /// HMAC I_L was invalid during key generation.
   InvalidKey,

   /// Attempted to derive the hardened child of an xpub
   HardenedDerivationFailed,

   /// Attempted to tweak an xpriv or xpub directly
   BadTweak,

   /// Child derivation would overflow the serialized depth byte
   DepthOverflow,

   /// Unrecognized version when deserializing xpriv
   BadXPrivVersionBytes([u8; 4]),

   /// Unrecognized version when deserializing xpub
   BadXPubVersionBytes([u8; 4]),

   /// Bad padding byte on serialized xprv
   BadPadding(u8),

   /// Bad Checks on b58check
   BadB58Checksum,

   /// Parsing an string derivation failed because an index string was malformatted
   MalformattedDerivation(String),

   /// Attempted to deserialize a DER signature to a recoverable signature.
   NoRecoveryId,

   /// Attempted to deserialize a very long path
   InvalidBip32Path,

   /// A caller-supplied error message
   Custom(String),
}

impl std::fmt::Display for Bip32Error {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      match self {
         Self::BackendError(_) => f.write_str("k256 error"),
         Self::EllipticCurveError(_) => f.write_str("elliptic curve error"),
         Self::IoError(e) => std::fmt::Display::fmt(e, f),
         Self::SecureError(e) => std::fmt::Display::fmt(e, f),
         Self::SeedTooShort => f.write_str("Master key seed generation received <16 bytes"),
         Self::InvalidKey => {
            f.write_str("HMAC left segment was 0 or greater than the curve order. How?")
         }
         Self::HardenedDerivationFailed => {
            f.write_str("Attempted to derive the hardened child of an xpub")
         }
         Self::BadTweak => f.write_str("Attempted to tweak an xpriv or xpub directly"),
         Self::DepthOverflow => f.write_str("Derivation depth exceeds 255"),
         Self::BadXPrivVersionBytes(bytes) => {
            write!(
               f,
               "Version bytes 0x{bytes:x?} don't match any network xpriv version bytes"
            )
         }
         Self::BadXPubVersionBytes(bytes) => {
            write!(
               f,
               "Version bytes 0x{bytes:x?} don't match any network xpub version bytes"
            )
         }
         Self::BadPadding(padding) => write!(f, "Expected 0 padding byte. Got {padding}"),
         Self::BadB58Checksum => f.write_str("Checksum mismatch on b58 deserialization"),
         Self::MalformattedDerivation(index) => {
            write!(f, "Malformatted index during derivation: {index}")
         }
         Self::NoRecoveryId => f.write_str(
            "Attempted to deserialize a DER signature to a recoverable signature. Use \
             deserialize_vrs instead",
         ),
         Self::InvalidBip32Path => f.write_str("Invalid Bip32 Path."),
         Self::Custom(message) => f.write_str(message),
      }
   }
}

impl std::error::Error for Bip32Error {
   fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
      match self {
         Self::IoError(e) => std::error::Error::source(e),
         _ => None,
      }
   }
}

impl From<std::io::Error> for Bip32Error {
   fn from(e: std::io::Error) -> Self {
      Self::IoError(e)
   }
}

impl From<secure_types::Error> for Bip32Error {
   fn from(e: secure_types::Error) -> Self {
      Self::SecureError(e)
   }
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
   fn from(i: std::convert::Infallible) -> Self {
      match i {}
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

      // The typed secure-types variant renders exactly what the old `Custom` stringification
      // of the inner error produced: the inner error's own text.
      assert_eq!(
         Bip32Error::from(secure_types::Error::LockFailed).to_string(),
         secure_types::Error::LockFailed.to_string()
      );

      // `#[error(transparent)]` handed `source()` the inner error's own source, and a plain
      // `std::io::Error` carries none. Pin the measured delegation, not a wrapping `Some`.
      let io = Bip32Error::IoError(std::io::Error::other("boom"));
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

      let nested = Bip32Error::IoError(std::io::Error::other(WithSource(Root)));
      assert_eq!(nested.to_string(), "outer");
      let source = std::error::Error::source(&nested).expect("transparent forwards the source");
      assert_eq!(source.to_string(), "root");

      assert!(std::error::Error::source(&Bip32Error::SeedTooShort).is_none());
      assert!(matches!(
         Bip32Error::from(std::io::Error::other("boom")),
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
