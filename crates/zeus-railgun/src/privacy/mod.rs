//! Unshield privacy: how linkable a withdrawal is, and what to do about it.
//!
//! A withdrawal's anonymity comes from looking like everyone else's. The cheapest signal an observer
//! has is an amount: a distinctive one, or one that matches a recent deposit (or a small sum of
//! them), links the withdrawal to that deposit without any other data. This module turns the
//! protocol activity Zeus already persists (the events snapshot) into the deposit set a check runs
//! against, scores a requested amount, and proposes a safer one.
//!
//! Nothing here does I/O: the caller supplies the events (or the deposits) and the clock, so the
//! whole thing is testable without a chain or a database. The scoring is a Rust port of the engine in
//! [railcheck](https://github.com/ddddubbby/railcheck) (MIT), adjusted where Zeus has better inputs
//! than a browser tool can: real deposit history instead of asked questions, and full wei precision
//! instead of a pool rounded to nano-ETH.
//!
//! The score is a model-based indication of amount-matching risk, **not** a measured probability of
//! identification and not a guarantee of anonymity. It says nothing about timing, IP addresses,
//! exchange records or the other off-chain data an observer may have.

pub mod activity;
pub mod assessment;

pub use activity::{Deposit, DepositWindow, deposits_from_events, estimated_timestamp};
pub use assessment::{
   ACTIVITY_WINDOW_SECONDS, MatchSets, PrivacyError, RiskBand, UnshieldAmountAdvice, UserExposure,
   assess_amount, user_exposure,
};
