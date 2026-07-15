#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

pub mod abis;
pub mod builder;
pub mod bundler;
pub mod entry_point;
pub mod signable_user_operation;
pub mod signed_user_operation;
pub mod smart_account;
pub mod user_operation;


#[cfg(not(target_arch = "wasm32"))]
pub async fn sleep(duration: web_time::Duration) {
    tokio::time::sleep(duration).await;
}

#[cfg(target_arch = "wasm32")]
pub async fn sleep(duration: web_time::Duration) {
    gloo_timers::future::sleep(duration).await;
}