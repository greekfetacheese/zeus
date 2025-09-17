use alloy_contract::private::Ethereum;
use alloy_json_rpc::{RequestPacket, ResponsePacket};
use alloy_provider::{
   Identity, ProviderBuilder, RootProvider, WsConnect,
   fillers::{BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller},
};
use alloy_rpc_client::ClientBuilder;
use alloy_transport::{
   TransportError, TransportErrorKind,
   layers::{RetryBackoffLayer, ThrottleLayer},
};
use tower::{BoxError, Layer, Service, timeout::Timeout};
use url::Url;

use std::{
   future::Future,
   pin::Pin,
   task::{Context, Poll},
   time::Duration,
};

pub type RpcClient = FillProvider<
   JoinFill<Identity, JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>>,
   RootProvider<Ethereum>,
>;

// Custom layer to apply timeout and map errors to TransportError
#[derive(Clone, Copy, Debug)]
struct TimeoutLayer(Duration);

impl TimeoutLayer {
   fn new(timeout: Duration) -> Self {
      Self(timeout)
   }
}

impl<S> Layer<S> for TimeoutLayer
where
   S: Service<RequestPacket> + Send + 'static,
   S::Future: Send + 'static,
{
   type Service = CustomTimeout<S>;

   fn layer(&self, inner: S) -> Self::Service {
      CustomTimeout(Timeout::new(inner, self.0))
   }
}

#[derive(Clone, Debug)]
struct CustomTimeout<S>(Timeout<S>);

impl<S> Service<RequestPacket> for CustomTimeout<S>
where
   S: Service<RequestPacket, Response = ResponsePacket, Error = TransportError> + Send + 'static,
   S::Future: Send + 'static,
{
   type Response = ResponsePacket;
   type Error = TransportError;
   type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

   fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
      self.0.poll_ready(cx).map_err(map_timeout_error)
   }

   fn call(&mut self, req: RequestPacket) -> Self::Future {
      let fut = self.0.call(req);
      Box::pin(async move { fut.await.map_err(map_timeout_error) })
   }
}

// Map BoxError (from Timeout) to TransportError
fn map_timeout_error(e: BoxError) -> TransportError {
   TransportErrorKind::custom_str(&format!("Request timeout {:?}", e)).into()
}

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
   timeout: u64,
) -> Result<RpcClient, anyhow::Error> {
   let is_ws = url.starts_with("ws");
   let url = Url::parse(url)?;
   let timeout = Duration::from_secs(timeout);

   let client_builder = ClientBuilder::default()
      .layer(retry_layer)
      .layer(throttle)
      .layer(TimeoutLayer::new(timeout));

   let client = if is_ws {
      client_builder.ws(WsConnect::new(url)).await?
   } else {
      client_builder.http(url)
   };

   let client = ProviderBuilder::new().connect_client(client);
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

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_provider::Provider;

   #[tokio::test]
   #[should_panic]
   async fn test_timeout() {
      let url = "wss://eth.merkle.io";
      let ws = WsConnect::new(url);
      let throttle = ThrottleLayer::new(5);
      let retry = RetryBackoffLayer::new(10, 400, 330);
      let timeout = Duration::from_millis(1);
      let client = ClientBuilder::default()
         .layer(throttle)
         .layer(retry)
         .layer(TimeoutLayer::new(timeout))
         .ws(ws)
         .await
         .unwrap();
      let client = ProviderBuilder::new().connect_client(client);

      let _block = client.get_block_number().await.unwrap();
   }
}
