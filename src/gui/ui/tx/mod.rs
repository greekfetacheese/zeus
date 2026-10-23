//! This module contains the UI components for showing a transaction
//!
//! - The TxConfirmationWindow contains as much information as possible about the transaction before the user confirms it.
//! - The TxWindow is what we show to the user for a transaction that has been confirmed.

pub mod confrim_window;
pub mod events;
pub mod modals;
pub mod rows;
pub mod spent_note_window;
pub mod tx_window;

pub use confrim_window::TxConfirmationWindow;
pub use modals::*;
pub use rows::*;
pub use spent_note_window::{SpentHistoryRow, SpentNoteWindow};
pub use tx_window::TxWindow;
