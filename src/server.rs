use crate::core::ZeusCtx;
use alloy_consensus::error;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::convert::Infallible;
use std::net::SocketAddr;
use tracing::{error, info, warn};
use warp::Filter;

use std::str::FromStr;
use zeus_eth::{
   alloy_network::TransactionBuilder,
   alloy_primitives::{Address, Bytes, hex},
   alloy_provider::Provider,
   alloy_rpc_types::TransactionRequest,
};

pub const SERVER_PORT: u16 = 65534;

// EIP-1193 Error codes
pub const USER_REJECTED_REQUEST: i32 = -4001;
pub const UNAUTHORIZED: i32 = -4100;
pub const UNSUPPORTED_METHOD: i32 = -4200;
pub const DISCONNECTED: i32 = -4900;
pub const CHAIN_DISCONNECTED: i32 = -4901;
pub const INVALID_PARAMS: i32 = -32602;
pub const INTERNAL_ERROR: i32 = -32603;

/// Eth methods we expect to receive from the extension/dapp
#[derive(Debug)]
pub enum EthMethod {
   EthAccounts,
   RequestAccounts,
   SendTransaction,
   Subscribe,
   Unsubscribe,
   BlockNumber,
   Call,
   ChainId,
   Coinbase,
   EstimateGas,
   FeeHistory,
   GasPrice,
   GetBalance,
   GetBlockByHash,
   GetBlockByNumber,
   GetBlockTransactionCountByHash,
   GetBlockTransactionCountByNumber,
   GetCode,
   GetFilterChanges,
   GetFilterLogs,
   GetLogs,
   GetProof,
   GetStorageAt,
   GetTransactionByBlockHashAndIndex,
   GetTransactionByBlockNumberAndIndex,
   GetTransactionByHash,
   GetTransactionCount,
   GetTransactionReceipt,
   NewBlockFilter,
   NewFilter,
   NewPendingTransactionFilter,
   SendRawTransaction,
}

impl EthMethod {
   pub fn as_str(&self) -> &'static str {
      match self {
         EthMethod::EthAccounts => "eth_accounts",
         EthMethod::RequestAccounts => "eth_requestAccounts",
         EthMethod::SendTransaction => "eth_sendTransaction",
         EthMethod::Subscribe => "eth_subscribe",
         EthMethod::Unsubscribe => "eth_unsubscribe",
         EthMethod::BlockNumber => "eth_blockNumber",
         EthMethod::Call => "eth_call",
         EthMethod::ChainId => "eth_chainId",
         EthMethod::Coinbase => "eth_coinbase",
         EthMethod::EstimateGas => "eth_estimateGas",
         EthMethod::FeeHistory => "eth_feeHistory",
         EthMethod::GasPrice => "eth_gasPrice",
         EthMethod::GetBalance => "eth_getBalance",
         EthMethod::GetBlockByHash => "eth_getBlockByHash",
         EthMethod::GetBlockByNumber => "eth_getBlockByNumber",
         EthMethod::GetBlockTransactionCountByHash => "eth_getBlockTransactionCountByHash",
         EthMethod::GetBlockTransactionCountByNumber => "eth_getBlockTransactionCountByNumber",
         EthMethod::GetCode => "eth_getCode",
         EthMethod::GetFilterChanges => "eth_getFilterChanges",
         EthMethod::GetFilterLogs => "eth_getFilterLogs",
         EthMethod::GetLogs => "eth_getLogs",
         EthMethod::GetProof => "eth_getProof",
         EthMethod::GetStorageAt => "eth_getStorageAt",
         EthMethod::GetTransactionByBlockHashAndIndex => "eth_getTransactionByBlockHashAndIndex",
         EthMethod::GetTransactionByBlockNumberAndIndex => "eth_getTransactionByBlockNumberAndIndex",
         EthMethod::GetTransactionByHash => "eth_getTransactionByHash",
         EthMethod::GetTransactionCount => "eth_getTransactionCount",
         EthMethod::GetTransactionReceipt => "eth_getTransactionReceipt",
         EthMethod::NewBlockFilter => "eth_newBlockFilter",
         EthMethod::NewFilter => "eth_newFilter",
         EthMethod::NewPendingTransactionFilter => "eth_newPendingTransactionFilter",
         EthMethod::SendRawTransaction => "eth_sendRawTransaction",
      }
   }

   pub fn supported_methods() -> Vec<EthMethod> {
      vec![
         EthMethod::EthAccounts,
         EthMethod::RequestAccounts,
         EthMethod::SendTransaction,
         EthMethod::Subscribe,
         EthMethod::Unsubscribe,
         EthMethod::BlockNumber,
         EthMethod::Call,
         EthMethod::ChainId,
         EthMethod::Coinbase,
         EthMethod::EstimateGas,
         EthMethod::FeeHistory,
         EthMethod::GasPrice,
         EthMethod::GetBalance,
         EthMethod::GetBlockByHash,
         EthMethod::GetBlockByNumber,
         EthMethod::GetBlockTransactionCountByHash,
         EthMethod::GetBlockTransactionCountByNumber,
         EthMethod::GetCode,
         EthMethod::GetFilterChanges,
         EthMethod::GetFilterLogs,
         EthMethod::GetLogs,
         EthMethod::GetProof,
         EthMethod::GetStorageAt,
         EthMethod::GetTransactionByBlockHashAndIndex,
         EthMethod::GetTransactionByBlockNumberAndIndex,
         EthMethod::GetTransactionByHash,
         EthMethod::GetTransactionCount,
         EthMethod::GetTransactionReceipt,
         EthMethod::NewBlockFilter,
         EthMethod::NewFilter,
         EthMethod::NewPendingTransactionFilter,
         EthMethod::SendRawTransaction,
      ]
   }
}

#[derive(Deserialize, Debug, Clone)]
struct ConnectionRequest {
   origin: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConnectionResponse {
   /// approved or rejected
   pub status: String,
   pub accounts: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StatusResponse {
   pub status: bool,
}

#[derive(Deserialize, Debug)]
/// Request received from the extension
struct JsonRpcRequest {
   jsonrpc: String,
   id: Value,
   method: String,
   #[serde(default)]
   params: Value,
}

#[derive(Serialize, Debug)]
/// Response sent back to the extension
struct JsonRpcResponse {
   jsonrpc: String,
   id: Value,
   #[serde(skip_serializing_if = "Option::is_none")]
   result: Option<Value>,
   #[serde(skip_serializing_if = "Option::is_none")]
   error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
   pub fn error(code: i32, payload_id: Value) -> Self {
      let error = JsonRpcError::from(code);
      Self {
         jsonrpc: "2.0".to_string(),
         id: payload_id,
         result: None,
         error: Some(error),
      }
   }
}

#[derive(Serialize, Debug)]
struct JsonRpcError {
   code: i32,
   message: String,
   #[serde(skip_serializing_if = "Option::is_none")]
   data: Option<Value>,
}

impl JsonRpcError {
   pub fn from(code: i32) -> Self {
      match code {
         USER_REJECTED_REQUEST => Self::user_rejected_request(),
         UNAUTHORIZED => Self::unauthorized(),
         UNSUPPORTED_METHOD => Self::unsupported_method(),
         DISCONNECTED => Self::disconnected(),
         CHAIN_DISCONNECTED => Self::chain_disconnected(),
         INVALID_PARAMS => Self::invalid_params(),
         INTERNAL_ERROR => Self::internal_error(),
         _ => Self::internal_error(),
      }
   }

   pub fn invalid_params() -> Self {
      Self {
         code: INVALID_PARAMS,
         message: "Invalid Params".to_string(),
         data: None,
      }
   }

   pub fn internal_error() -> Self {
      Self {
         code: INTERNAL_ERROR,
         message: "Internal Error".to_string(),
         data: None,
      }
   }

   pub fn user_rejected_request() -> Self {
      Self {
         code: USER_REJECTED_REQUEST,
         message: "User Rejected Request".to_string(),
         data: None,
      }
   }

   pub fn unauthorized() -> Self {
      Self {
         code: UNAUTHORIZED,
         message: "Unauthorized".to_string(),
         data: None,
      }
   }

   pub fn unsupported_method() -> Self {
      Self {
         code: UNSUPPORTED_METHOD,
         message: "Unsupported Method".to_string(),
         data: None,
      }
   }

   pub fn chain_disconnected() -> Self {
      Self {
         code: CHAIN_DISCONNECTED,
         message: "Chain Disconnected".to_string(),
         data: None,
      }
   }

   pub fn disconnected() -> Self {
      Self {
         code: DISCONNECTED,
         message: "Disconnected".to_string(),
         data: None,
      }
   }
}

/// Request a wallet connection
async fn request_connection(ctx: ZeusCtx, payload: ConnectionRequest) -> Result<impl warp::Reply, Infallible> {
   info!("POST /request-connection for origin: {}", payload.origin);

   // TODO: prompt confirm in the GUI
   // for now approve the connection

   let wallets = ctx.wallets_info();
   let mut accounts = Vec::new();
   for wallet in wallets {
      accounts.push(wallet.address_string());
   }
   info!("Connection approved by user");

   let response = ConnectionResponse {
      status: "approved".to_string(),
      accounts,
   };

   Ok(warp::reply::with_status(
      warp::reply::json(&response),
      warp::http::StatusCode::OK,
   ))
}

// Handler for GET /status
async fn status_handler(_ctx: ZeusCtx) -> Result<impl warp::Reply, Infallible> {
   info!("GET /status requested");

   let response = StatusResponse { status: true };

   Ok(warp::reply::json(&response))
}

fn request_accounts(ctx: ZeusCtx, payload: JsonRpcRequest) -> Result<JsonRpcResponse, Infallible> {
   let wallets = ctx.wallets_info();
   let mut accounts = Vec::new();
   for wallet in wallets {
      accounts.push(wallet.address_string());
   }

   let response = JsonRpcResponse {
      jsonrpc: "2.0".to_string(),
      id: payload.id,
      result: Some(json!(accounts)),
      error: None,
   };

   Ok(response)
}

fn chain_id(ctx: ZeusCtx, payload: JsonRpcRequest) -> Result<JsonRpcResponse, Infallible> {
   let chain_id = ctx.chain().id_as_hex();

   let response = JsonRpcResponse {
      jsonrpc: "2.0".to_string(),
      id: payload.id,
      result: Some(json!(chain_id)),
      error: None,
   };

   Ok(response)
}

async fn block_number(ctx: ZeusCtx, payload: JsonRpcRequest) -> Result<JsonRpcResponse, Infallible> {
   // For now just get the client for the current Ctx chain
   let chain = ctx.chain().id();
   let client = ctx.get_client_with_id(chain).unwrap();
   let block_number = client.get_block_number().await.unwrap_or(0);
   info!("Block number: {}", block_number);

   let response = JsonRpcResponse {
      jsonrpc: "2.0".to_string(),
      id: payload.id,
      result: Some(json!(block_number)),
      error: None,
   };

   Ok(response)
}

fn get_balance(ctx: ZeusCtx, payload: JsonRpcRequest) -> Result<JsonRpcResponse, Infallible> {
   let params_array = match payload.params {
      Value::Array(params) => params,
      _ => return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id)),
   };

   let address_str = match params_array.get(0) {
      Some(Value::String(address)) => address,
      _ => return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id)),
   };

   let address = match Address::from_str(address_str) {
      Ok(address) => address,
      Err(_) => return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id)),
   };

   let chain = ctx.chain().id();
   let balance = ctx.get_eth_balance(chain, address).unwrap_or_default();

   let response = JsonRpcResponse {
      jsonrpc: "2.0".to_string(),
      id: payload.id,
      result: Some(json!(balance.wei().unwrap())),
      error: None,
   };

   Ok(response)
}

async fn eth_call(ctx: ZeusCtx, payload: JsonRpcRequest) -> Result<JsonRpcResponse, Infallible> {
   // params is an array which contain one object and one string
   let params_array = match payload.params {
      Value::Array(params) => params,
      _ => {
         return {
            error!(
               "Invalid params for eth_call, params is not an array {:#?}",
               payload.params
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
   };

   let params_object = match params_array.get(0) {
      Some(Value::Object(params)) => params,
      _ => {
         return {
            error!(
               "Invalid params for eth_call, params[0] is not an object {:#?}",
               params_array
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
   };

   let (calldata_str, to_str) = match (params_object.get("data"), params_object.get("to")) {
      (Some(Value::String(calldata)), Some(Value::String(to))) => (calldata, to),
      _ => {
         return {
            error!(
               "Invalid params for eth_call, data and to are not strings {:#?}",
               params_object
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
   };

   let to = match Address::from_str(to_str) {
      Ok(to) => to,
      Err(_) => {
         return {
            error!(
               "Invalid params for eth_call, String is not a valid ethereum address {:#?}",
               to_str
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
   };

   let calldata = match Bytes::from_str(calldata_str) {
      Ok(calldata) => calldata,
      Err(_) => {
         return {
            error!(
               "Invalid params for eth_call, String is not valid bytes {:#?}",
               calldata_str
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
   };

   let chain = ctx.chain().id();
   let client = ctx.get_client_with_id(chain).unwrap();
   let from = ctx.current_wallet().address;

   let tx = TransactionRequest::default()
      .with_from(from)
      .with_to(to)
      .with_input(calldata);

   let output = match client.call(tx).await {
      Ok(output) => output,
      Err(_) => return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id)),
   };

   // convert to hex string
   let result = hex::encode(output);
   info!("Eth Call Result: {}", result);

   let response = JsonRpcResponse {
      jsonrpc: "2.0".to_string(),
      id: payload.id,
      result: Some(json!(result)),
      error: None,
   };

   Ok(response)
}

async fn handle_request(ctx: ZeusCtx, payload: JsonRpcRequest) -> Result<JsonRpcResponse, Infallible> {
   let method = payload.method.as_str();
   let res_body = if method == EthMethod::RequestAccounts.as_str() || method == EthMethod::EthAccounts.as_str() {
      request_accounts(ctx, payload)?
   } else if payload.method.as_str() == EthMethod::ChainId.as_str() {
      chain_id(ctx, payload)?
   } else if payload.method.as_str() == EthMethod::BlockNumber.as_str() {
      block_number(ctx, payload).await?
   } else if method == EthMethod::GetBalance.as_str() {
      get_balance(ctx, payload)?
   } else if method == EthMethod::Call.as_str() {
      eth_call(ctx, payload).await?
   } else {
      // TODO
      JsonRpcResponse {
         jsonrpc: "2.0".to_string(),
         id: payload.id,
         result: None,
         error: Some(JsonRpcError {
            code: -32601, // Method not found
            message: format!("Method '{}' not supported.", payload.method),
            data: None,
         }),
      }
   };
   Ok(res_body)
}

// Handler for POST /api (JSON-RPC)
async fn api_handler(ctx: ZeusCtx, payload: JsonRpcRequest) -> Result<impl warp::Reply, Infallible> {
   info!("POST /api requested with method: {}", payload.method);

   let response_body = handle_request(ctx, payload).await?;

   Ok(warp::reply::json(&response_body))
}

fn with_ctx(ctx: ZeusCtx) -> impl Filter<Extract = (ZeusCtx,), Error = Infallible> + Clone {
   warp::any().map(move || ctx.clone())
}

pub async fn run_server(ctx: ZeusCtx) -> Result<(), Box<dyn std::error::Error>> {
   let cors = warp::cors()
      .allow_any_origin()
      .allow_methods(vec!["GET", "POST", "OPTIONS"])
      .allow_headers(vec!["Content-Type", "Accept"]);

   // Filter for GET /status
   let status_route = warp::path!("status")
      .and(warp::get())
      .and(with_ctx(ctx.clone()))
      .and_then(status_handler);

   // Filter for POST /api
   let api_route = warp::path!("api")
      .and(warp::post())
      .and(with_ctx(ctx.clone()))
      .and(warp::body::json::<JsonRpcRequest>()) // payload
      .and_then(api_handler);

   let request_connection_route = warp::path!("request-connection")
      .and(warp::post())
      .and(with_ctx(ctx.clone()))
      .and(warp::body::json::<ConnectionRequest>())
      .and_then(request_connection);

   // --- Combine Routes ---
   let routes = status_route
      .or(api_route)
      .or(request_connection_route)
      .with(cors)
      .with(warp::trace::request());

   let addr = SocketAddr::from(([127, 0, 0, 1], SERVER_PORT));
   info!("Zeus (warp) RPC server listening on {}", addr);

   warp::serve(routes).run(addr).await;

   Ok(())
}
