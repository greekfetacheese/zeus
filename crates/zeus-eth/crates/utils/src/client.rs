use alloy_contract::private::Ethereum;
use alloy_provider::{
   Identity, ProviderBuilder, RootProvider,
   fillers::{BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller},
};
use alloy_rpc_client::ClientBuilder;
use alloy_transport::layers::{RetryBackoffLayer, ThrottleLayer};
use std::sync::Arc;
use url::Url;

pub type HttpClient = Arc<
   FillProvider<
      JoinFill<Identity, JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>>,
      RootProvider<Ethereum>,
   >,
>;

pub fn retry_layer(
   max_rate_limit_retries: u32,
   initial_backoff: u64,
   compute_units_per_second: u64,
) -> RetryBackoffLayer {
   RetryBackoffLayer::new(
      max_rate_limit_retries,
      initial_backoff,
      compute_units_per_second,
   )
}

pub fn throttle_layer(max_requests_per_second: u32) -> ThrottleLayer {
   ThrottleLayer::new(max_requests_per_second)
}

pub fn get_http_client(
   url: &str,
   retry_layer: RetryBackoffLayer,
   throttle: ThrottleLayer,
) -> Result<HttpClient, anyhow::Error> {
   let url = Url::parse(url)?;
   let client = ClientBuilder::default()
      .layer(retry_layer)
      .layer(throttle)
      .http(url);
   let client = Arc::new(ProviderBuilder::new().on_client(client));

   Ok(client)
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_provider::Provider;

   #[tokio::test]
   async fn test_throttle() {
      let retry = RetryBackoffLayer::new(10, 300, 330);
      let throttle = ThrottleLayer::new(2);
      let url = "https://eth.merkle.io";
      let client = get_http_client(url, retry, throttle).unwrap();

      let mut handles = Vec::new();
      for _ in 0..20 {
         let client = client.clone();
         let handle = tokio::spawn(async move {
            let block = client.get_block_number().await.unwrap();
            println!("Block: {}", block);
         });
         handles.push(handle);
      }

      for handle in handles {
         handle.await.unwrap();
      }
   }
}
