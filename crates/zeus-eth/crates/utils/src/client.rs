use alloy_contract::private::Ethereum;
use alloy_provider::{
   Identity, ProviderBuilder, RootProvider,
   fillers::{BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller},
};
use std::sync::Arc;
use url::Url;

pub type HttpClient = Arc<
   FillProvider<
      JoinFill<Identity, JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>>,
      RootProvider<Ethereum>,
   >,
>;

pub fn get_http_client(url: &str) -> Result<HttpClient, anyhow::Error> {
   let url = Url::parse(url)?;
   let client = Arc::new(ProviderBuilder::new().on_http(url));

   Ok(client)
}

mod tests {
   #![allow(unused_imports)]
   use super::get_http_client;
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
}
