use futures::channel::mpsc::{SendError, TrySendError};
use revm::database_interface::DBErrorMarker;
use std::fmt;
use std::sync::{Arc, mpsc::RecvError};

#[allow(dead_code)]
// Errors that can happen when working with [`revm::Database`]
#[derive(Debug)]
#[non_exhaustive]
pub enum DatabaseError {
   MissingAccount(revm::primitives::Address),
   MissingCode(revm::primitives::B256),
   Recv(RecvError),
   Send(SendError),
   Message(String),
   GetAccount(revm::primitives::Address, Arc<anyhow::Error>),
   GetStorage(
      revm::primitives::Address,
      revm::primitives::U256,
      Arc<anyhow::Error>,
   ),
   GetBlockHash(revm::primitives::U256, Arc<anyhow::Error>),
   BackendFetchRequestError,
   ChannelRecvError,
}

impl fmt::Display for DatabaseError {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      match self {
         DatabaseError::MissingAccount(address) => {
            write!(f, "Failed to fetch AccountInfo {address:?}")
         }
         DatabaseError::MissingCode(code_hash) => {
            write!(f, "Could should already be loaded: {code_hash:?}")
         }
         // `transparent`: the inner error's own Display, nothing added.
         DatabaseError::Recv(err) => err.fmt(f),
         DatabaseError::Send(err) => err.fmt(f),
         DatabaseError::Message(msg) => f.write_str(msg),
         DatabaseError::GetAccount(address, cause) => {
            write!(
               f,
               "Failed to get account for {address:?}: {cause:?}"
            )
         }
         DatabaseError::GetStorage(address, index, cause) => {
            write!(
               f,
               "Failed to get storage for {address:?} at {index:?}: {cause:?}"
            )
         }
         DatabaseError::GetBlockHash(number, cause) => {
            write!(
               f,
               "Failed to get block hash for {number}: {cause:?}"
            )
         }
         DatabaseError::BackendFetchRequestError => f.write_str("Backend Request Error"),
         DatabaseError::ChannelRecvError => f.write_str("Channel recv error"),
      }
   }
}

impl std::error::Error for DatabaseError {
   fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
      match self {
         // `transparent` forwards the inner error's source rather than returning the inner itself.
         DatabaseError::Recv(err) => err.source(),
         DatabaseError::Send(err) => err.source(),
         _ => None,
      }
   }
}

impl From<RecvError> for DatabaseError {
   fn from(err: RecvError) -> Self {
      DatabaseError::Recv(err)
   }
}

impl From<SendError> for DatabaseError {
   fn from(err: SendError) -> Self {
      DatabaseError::Send(err)
   }
}

impl<T> From<TrySendError<T>> for DatabaseError {
   fn from(err: TrySendError<T>) -> Self {
      err.into_send_error().into()
   }
}

#[allow(dead_code)]
impl DatabaseError {
   // Create a new error with a message
   pub fn msg(msg: impl Into<String>) -> Self {
      DatabaseError::Message(msg.into())
   }
}

impl DBErrorMarker for DatabaseError {}

// Result alias with `DatabaseError` as error
pub type DatabaseResult<T> = Result<T, DatabaseError>;

#[cfg(test)]
mod tests {
   use super::*;
   use futures::channel::mpsc::channel;
   use revm::primitives::{Address, B256, U256};
   use std::error::Error as _;

   fn address() -> Address {
      Address::from([0x11; 20])
   }

   fn cause() -> Arc<anyhow::Error> {
      Arc::new(anyhow::anyhow!("boom"))
   }

   fn closed_channel_send_error() -> SendError {
      let (mut tx, rx) = channel::<u8>(1);
      drop(rx);
      match tx.try_send(0) {
         Err(e) => e.into_send_error(),
         Ok(()) => panic!("a closed channel must not accept a send"),
      }
   }

   // Every message is pinned byte-exact: they are the strings a user sees in a wallet error.
   #[test]
   fn display_matches_the_rendered_messages() {
      assert_eq!(
         DatabaseError::MissingAccount(address()).to_string(),
         "Failed to fetch AccountInfo 0x1111111111111111111111111111111111111111"
      );
      assert_eq!(
         DatabaseError::MissingCode(B256::from([0x01; 32])).to_string(),
         "Could should already be loaded: \
          0x0101010101010101010101010101010101010101010101010101010101010101"
      );
      assert_eq!(
         DatabaseError::Recv(RecvError).to_string(),
         "receiving on a closed channel"
      );
      assert_eq!(
         DatabaseError::Send(closed_channel_send_error()).to_string(),
         "send failed because receiver is gone"
      );
      assert_eq!(
         DatabaseError::Message("boom".to_string()).to_string(),
         "boom"
      );
      assert_eq!(
         DatabaseError::GetAccount(address(), cause()).to_string(),
         "Failed to get account for \
          0x1111111111111111111111111111111111111111: boom"
      );
      assert_eq!(
         DatabaseError::GetStorage(address(), U256::from(42), cause()).to_string(),
         "Failed to get storage for \
          0x1111111111111111111111111111111111111111 at 42: boom"
      );
      assert_eq!(
         DatabaseError::GetBlockHash(U256::from(42), cause()).to_string(),
         "Failed to get block hash for 42: boom"
      );
      assert_eq!(
         DatabaseError::BackendFetchRequestError.to_string(),
         "Backend Request Error"
      );
      assert_eq!(
         DatabaseError::ChannelRecvError.to_string(),
         "Channel recv error"
      );
   }

   // `Recv` and `Send` were `transparent`, so Display forwards to the inner error — and so does
   // `source()`, which forwards the *inner's own* source rather than answering `Some(inner)`. Both
   // inner types implement `Error` with no source, so the rendered chain stops here.
   #[test]
   fn recv_and_send_delegate_display_and_have_no_further_source() {
      let inner = RecvError;
      let err = DatabaseError::Recv(inner);
      assert_eq!(err.to_string(), inner.to_string());
      assert!(
         inner.source().is_none(),
         "std's RecvError has no source of its own"
      );
      assert!(
         err.source().is_none(),
         "transparent forwards the inner error's source, it does not answer Some(inner)"
      );

      let inner = closed_channel_send_error();
      let inner_display = inner.to_string();
      assert!(
         inner.source().is_none(),
         "futures' SendError has no source of its own"
      );
      let err = DatabaseError::Send(inner);
      assert_eq!(err.to_string(), inner_display);
      assert!(
         err.source().is_none(),
         "transparent forwards the inner error's source, it does not answer Some(inner)"
      );
   }

   // No other variant had a `#[from]`/`transparent` field, so none of them has a source.
   #[test]
   fn every_other_variant_has_no_source() {
      let errors = [
         DatabaseError::MissingAccount(address()),
         DatabaseError::MissingCode(B256::ZERO),
         DatabaseError::Message("boom".to_string()),
         DatabaseError::GetAccount(address(), cause()),
         DatabaseError::GetStorage(address(), U256::ZERO, cause()),
         DatabaseError::GetBlockHash(U256::ZERO, cause()),
         DatabaseError::BackendFetchRequestError,
         DatabaseError::ChannelRecvError,
      ];

      for err in errors {
         assert!(
            err.source().is_none(),
            "{err:?} must not expose a source"
         );
      }
   }

   // The hand-written `From<TrySendError<T>>` is not a `#[from]`; it maps a failed send onto `Send`.
   #[test]
   fn try_send_error_lands_in_send() {
      let (mut tx, rx) = channel::<u8>(1);
      drop(rx);
      let try_send = match tx.try_send(0) {
         Err(e) => e,
         Ok(()) => panic!("a closed channel must not accept a send"),
      };

      let err: DatabaseError = try_send.into();
      assert!(
         matches!(err, DatabaseError::Send(_)),
         "a TrySendError must convert to DatabaseError::Send, got {err:?}"
      );
   }

   // `#[from]` generated these; they must keep landing in the right variant.
   #[test]
   fn from_impls_land_in_the_right_variant() {
      let err: DatabaseError = RecvError.into();
      assert!(
         matches!(err, DatabaseError::Recv(_)),
         "RecvError must convert to DatabaseError::Recv, got {err:?}"
      );

      let err: DatabaseError = closed_channel_send_error().into();
      assert!(
         matches!(err, DatabaseError::Send(_)),
         "SendError must convert to DatabaseError::Send, got {err:?}"
      );
   }
}
