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

pub fn get_http_client(url: &str) -> Result<HttpClient, anyhow::Error> {
   let retry_layer = RetryBackoffLayer::new(10, 300, 330);
   let url = Url::parse(url)?;
   let client = ClientBuilder::default().layer(retry_layer).http(url);
   let client = Arc::new(ProviderBuilder::new().on_client(client));

   Ok(client)
}

pub fn get_http_client_with_throttle(url: &str) -> Result<HttpClient, anyhow::Error> {
   let request_per_second = 10;
   let throttle_layer = ThrottleLayer::new(request_per_second);
   let retry_layer = RetryBackoffLayer::new(10, 300, 330);

   let url = Url::parse(url)?;
   let client = ClientBuilder::default()
      .layer(throttle_layer)
      .layer(retry_layer)
      .http(url);

   let client = Arc::new(ProviderBuilder::new().on_client(client));

   Ok(client)
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::price_feed::get_eth_price;
   use alloy_provider::Provider;

   #[tokio::test]
   async fn test_http_client() {
      let url = "https://eth.merkle.io";
      let client = get_http_client(url).unwrap();
      let block = client.get_block_number().await.unwrap();
      let price = get_eth_price(client, 1, None).await.unwrap();
      println!("ETH Price: {}", price);
      println!("Block: {}", block);
   }

   #[tokio::test]
   async fn test_throttle() {
      let url = "https://eth.merkle.io";
      let client = get_http_client_with_throttle(url).unwrap();

      let mut handles = Vec::new();
      for _ in 0..10 {
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
