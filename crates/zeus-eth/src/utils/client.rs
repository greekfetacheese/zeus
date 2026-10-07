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
   JoinFill<
      Identity,
      JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
   >,
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

/// Default request timeout (seconds) when a caller does not set one.
const DEFAULT_TIMEOUT_SECS: u64 = 15;
/// Default max rate-limit retries for the retry layer.
const DEFAULT_MAX_RATE_LIMIT_RETRIES: u32 = 10;
/// Default retry backoff base (ms).
const DEFAULT_INITIAL_BACKOFF: u64 = 400;
/// Default compute-unit budget per second.
const DEFAULT_COMPUTE_UNITS_PER_SECOND: u64 = 330;
/// Default max requests per second for the throttle layer.
const DEFAULT_MAX_REQUESTS_PER_SECOND: u32 = 10;
/// Default websocket reconnect budget.
const DEFAULT_WS_MAX_RETRIES: u32 = 10;
/// Default websocket reconnect backoff base.
const DEFAULT_WS_RETRY_INTERVAL: Duration = Duration::from_secs(3);

/// Builder for a connected [`RpcClient`].
///
/// Websocket endpoints dial immediately in [`RpcClientBuilder::connect`]; http endpoints are lazy and
/// do no I/O until the first request.
#[must_use = "builders do nothing unless you call connect()"]
pub struct RpcClientBuilder {
   url: String,
   retry: RetryBackoffLayer,
   throttle: ThrottleLayer,
   timeout: Duration,
   ws_max_retries: u32,
   ws_retry_interval: Duration,
}

impl RpcClientBuilder {
   pub fn new(url: impl Into<String>) -> Self {
      Self {
         url: url.into(),
         retry: retry_layer(
            DEFAULT_MAX_RATE_LIMIT_RETRIES,
            DEFAULT_INITIAL_BACKOFF,
            DEFAULT_COMPUTE_UNITS_PER_SECOND,
         ),
         throttle: throttle_layer(DEFAULT_MAX_REQUESTS_PER_SECOND),
         timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
         ws_max_retries: DEFAULT_WS_MAX_RETRIES,
         ws_retry_interval: DEFAULT_WS_RETRY_INTERVAL,
      }
   }

   #[must_use]
   pub fn retry(mut self, retry: RetryBackoffLayer) -> Self {
      self.retry = retry;
      self
   }

   #[must_use]
   pub fn throttle(mut self, throttle: ThrottleLayer) -> Self {
      self.throttle = throttle;
      self
   }

   #[must_use]
   pub fn timeout_secs(mut self, secs: u64) -> Self {
      self.timeout = Duration::from_secs(secs);
      self
   }

   /// Reconnect budget for websocket endpoints. Use [`u32::MAX`] to retry forever.
   #[must_use]
   pub fn ws_max_retries(mut self, max_retries: u32) -> Self {
      self.ws_max_retries = max_retries;
      self
   }

   /// Base interval for the websocket reconnect backoff (alloy caps the delay at 30s).
   #[must_use]
   pub fn ws_retry_interval(mut self, interval: Duration) -> Self {
      self.ws_retry_interval = interval;
      self
   }

   pub async fn connect(self) -> Result<RpcClient, anyhow::Error> {
      let is_ws = self.url.starts_with("ws");
      let url = Url::parse(&self.url)?;

      let client_builder = ClientBuilder::default()
         .layer(self.retry)
         .layer(self.throttle)
         .layer(TimeoutLayer::new(self.timeout));

      let client = if is_ws {
         let ws = WsConnect::new(self.url)
            .with_max_retries(self.ws_max_retries)
            .with_retry_interval(self.ws_retry_interval);
         client_builder.ws(ws).await?
      } else {
         client_builder.http(url)
      };

      Ok(ProviderBuilder::new().connect_client(client))
   }
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
