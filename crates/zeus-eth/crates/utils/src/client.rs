use alloy_contract::private::Ethereum;
use alloy_provider::{
   Identity, ProviderBuilder, RootProvider, WsConnect,
   fillers::{BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller},
};
use alloy_rpc_client::ClientBuilder;
use alloy_transport::layers::{RetryBackoffLayer, ThrottleLayer};
use url::Url;

pub type RpcClient = FillProvider<
   JoinFill<Identity, JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>>,
   RootProvider<Ethereum>,
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

pub async fn get_client(
   url: &str,
   retry_layer: RetryBackoffLayer,
   throttle: ThrottleLayer,
) -> Result<RpcClient, anyhow::Error> {
   let is_ws = url.starts_with("ws");
   let client = if is_ws {
      get_ws_client(url, retry_layer, throttle).await?
   } else {
      get_http_client(url, retry_layer, throttle)?
   };
   Ok(client)
}

pub fn get_http_client(
   url: &str,
   retry_layer: RetryBackoffLayer,
   throttle: ThrottleLayer,
) -> Result<RpcClient, anyhow::Error> {
   let url = Url::parse(url)?;
   let client = ClientBuilder::default()
      .layer(retry_layer)
      .layer(throttle)
      .http(url);
   let client = ProviderBuilder::new().connect_client(client);
   Ok(client)
}

pub async fn get_ws_client(
   url: &str,
   retry_layer: RetryBackoffLayer,
   throttle: ThrottleLayer,
) -> Result<RpcClient, anyhow::Error> {
   let url = Url::parse(url)?;
   let client = ClientBuilder::default()
      .layer(retry_layer)
      .layer(throttle)
      .ws(WsConnect::new(url))
      .await?;
   let client = ProviderBuilder::new().connect_client(client);
   Ok(client)
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_provider::Provider;

   #[tokio::test]
   async fn test_throttle_ws() {
      let url = "wss://eth.merkle.io";
      let ws = WsConnect::new(url);
      let throttle = ThrottleLayer::new(5);
      let retry = RetryBackoffLayer::new(10, 400, 330);
      let client = ClientBuilder::default()
         .layer(throttle)
         .layer(retry)
         .ws(ws)
         .await
         .unwrap();
      let client = ProviderBuilder::new().connect_client(client);

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
