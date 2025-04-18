use crate::core::utils::{action::OnChainAction, update};
use crate::core::{ZeusCtx, utils::tx};
use crate::gui::SHARED_GUI;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::convert::Infallible;
use std::future::IntoFuture;
use std::net::SocketAddr;
use tracing::{error, info};
use warp::Filter;

use std::str::FromStr;
use zeus_eth::{
   alloy_network::TransactionBuilder,
   alloy_primitives::{Address, Bytes, TxKind, U256, hex},
   alloy_provider::Provider,
   alloy_rpc_types::{BlockId, TransactionRequest},
   currency::{Currency, NativeCurrency},
   revm_utils::{ExecuteCommitEvm, ForkFactory, Host, new_evm, revert_msg},
   types::ChainId,
   utils::NumericValue,
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

/// Type of a request we expect to receive from the extension/dapp
///
/// This includes both EIP-1193 and wallet requests
#[derive(Debug)]
pub enum RequestMethod {
   WalletAddEthereumChain,
   WallletSwitchEthereumChain,
   WalletGetPermissions,
   WalletRequestPermissions,
   WalletRevokePermissions,
   EthAccounts,
   RequestAccounts,
   SendTransaction,
   Subscribe,
   Unsubscribe,
   BlockNumber,
   EthCall,
   ChainId,
   Coinbase,
   EstimateGas,
   FeeHistory,
   EthGasPrice,
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

impl RequestMethod {
   pub fn as_str(&self) -> &'static str {
      match self {
         RequestMethod::WalletAddEthereumChain => "wallet_addEthereumChain",
         RequestMethod::WallletSwitchEthereumChain => "wallet_switchEthereumChain",
         RequestMethod::WalletGetPermissions => "wallet_getPermissions",
         RequestMethod::WalletRequestPermissions => "wallet_requestPermissions",
         RequestMethod::WalletRevokePermissions => "wallet_revokePermissions",
         RequestMethod::EthAccounts => "eth_accounts",
         RequestMethod::RequestAccounts => "eth_requestAccounts",
         RequestMethod::SendTransaction => "eth_sendTransaction",
         RequestMethod::Subscribe => "eth_subscribe",
         RequestMethod::Unsubscribe => "eth_unsubscribe",
         RequestMethod::BlockNumber => "eth_blockNumber",
         RequestMethod::EthCall => "eth_call",
         RequestMethod::ChainId => "eth_chainId",
         RequestMethod::Coinbase => "eth_coinbase",
         RequestMethod::EstimateGas => "eth_estimateGas",
         RequestMethod::FeeHistory => "eth_feeHistory",
         RequestMethod::EthGasPrice => "eth_gasPrice",
         RequestMethod::GetBalance => "eth_getBalance",
         RequestMethod::GetBlockByHash => "eth_getBlockByHash",
         RequestMethod::GetBlockByNumber => "eth_getBlockByNumber",
         RequestMethod::GetBlockTransactionCountByHash => "eth_getBlockTransactionCountByHash",
         RequestMethod::GetBlockTransactionCountByNumber => "eth_getBlockTransactionCountByNumber",
         RequestMethod::GetCode => "eth_getCode",
         RequestMethod::GetFilterChanges => "eth_getFilterChanges",
         RequestMethod::GetFilterLogs => "eth_getFilterLogs",
         RequestMethod::GetLogs => "eth_getLogs",
         RequestMethod::GetProof => "eth_getProof",
         RequestMethod::GetStorageAt => "eth_getStorageAt",
         RequestMethod::GetTransactionByBlockHashAndIndex => {
            "eth_getTransactionByBlockHashAndIndex"
         }
         RequestMethod::GetTransactionByBlockNumberAndIndex => {
            "eth_getTransactionByBlockNumberAndIndex"
         }
         RequestMethod::GetTransactionByHash => "eth_getTransactionByHash",
         RequestMethod::GetTransactionCount => "eth_getTransactionCount",
         RequestMethod::GetTransactionReceipt => "eth_getTransactionReceipt",
         RequestMethod::NewBlockFilter => "eth_newBlockFilter",
         RequestMethod::NewFilter => "eth_newFilter",
         RequestMethod::NewPendingTransactionFilter => "eth_newPendingTransactionFilter",
         RequestMethod::SendRawTransaction => "eth_sendRawTransaction",
      }
   }

   pub fn supported_methods() -> Vec<RequestMethod> {
      vec![
         RequestMethod::WalletAddEthereumChain,
         RequestMethod::WallletSwitchEthereumChain,
         RequestMethod::WalletGetPermissions,
         RequestMethod::WalletRequestPermissions,
         RequestMethod::WalletRevokePermissions,
         RequestMethod::EthAccounts,
         RequestMethod::RequestAccounts,
         RequestMethod::SendTransaction,
         RequestMethod::Subscribe,
         RequestMethod::Unsubscribe,
         RequestMethod::BlockNumber,
         RequestMethod::EthCall,
         RequestMethod::ChainId,
         RequestMethod::Coinbase,
         RequestMethod::EstimateGas,
         RequestMethod::FeeHistory,
         RequestMethod::EthGasPrice,
         RequestMethod::GetBalance,
         RequestMethod::GetBlockByHash,
         RequestMethod::GetBlockByNumber,
         RequestMethod::GetBlockTransactionCountByHash,
         RequestMethod::GetBlockTransactionCountByNumber,
         RequestMethod::GetCode,
         RequestMethod::GetFilterChanges,
         RequestMethod::GetFilterLogs,
         RequestMethod::GetLogs,
         RequestMethod::GetProof,
         RequestMethod::GetStorageAt,
         RequestMethod::GetTransactionByBlockHashAndIndex,
         RequestMethod::GetTransactionByBlockNumberAndIndex,
         RequestMethod::GetTransactionByHash,
         RequestMethod::GetTransactionCount,
         RequestMethod::GetTransactionReceipt,
         RequestMethod::NewBlockFilter,
         RequestMethod::NewFilter,
         RequestMethod::NewPendingTransactionFilter,
         RequestMethod::SendRawTransaction,
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
   origin: String,
   #[allow(dead_code)]
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

   pub fn ok(result: Option<Value>, payload_id: Value) -> Self {
      Self {
         jsonrpc: "2.0".to_string(),
         id: payload_id,
         result,
         error: None,
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
async fn request_connection(
   ctx: ZeusCtx,
   payload: ConnectionRequest,
) -> Result<impl warp::Reply, Infallible> {
   info!(
      "request-connection for origin: {}",
      payload.origin
   );

   // open the confirmation window
   SHARED_GUI.write(|gui| {
      gui.confirm_window.open("Connect to Dapp");
      gui.confirm_window.set_msg2(payload.origin);
   });

   // wait for the user to confirm or reject the connection
   let mut confirmed = None;
   loop {
      tokio::time::sleep(std::time::Duration::from_millis(100)).await;

      SHARED_GUI.read(|gui| {
         confirmed = gui.confirm_window.confirm;
      });

      if confirmed.is_some() {
         SHARED_GUI.write(|gui| {
            gui.confirm_window.reset();
         });
         break;
      }
   }

   let confirmed = confirmed.unwrap();
   if !confirmed {
      return Ok(warp::reply::with_status(
         warp::reply::json(&json!({ "status": "rejected" })),
         warp::http::StatusCode::OK,
      ));
   }

   let current_wallet = ctx.current_wallet();
   info!("Connection approved by user");

   let response = ConnectionResponse {
      status: "approved".to_string(),
      accounts: vec![current_wallet.address_string()],
   };

   Ok(warp::reply::with_status(
      warp::reply::json(&response),
      warp::http::StatusCode::OK,
   ))
}

// Handler for GET /status
async fn status_handler(ctx: ZeusCtx) -> Result<impl warp::Reply, Infallible> {
   let chain = ctx.chain().id();
   let chain = format!("0x{:x}", chain);
   let current_wallet = ctx.current_wallet();

   let res = json!({
    "status": true,
    "accounts": vec![current_wallet.address_string()],
    "chainId": chain,
   });

   Ok(warp::reply::json(&res))
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

async fn block_number(
   ctx: ZeusCtx,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let chain = ctx.chain().id();
   let client = ctx.get_client_with_id(chain).unwrap();
   let block_number = client.get_block_number().await.unwrap_or(0);

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
      _ => {
         return {
            error!(
               "Invalid params for eth_getBalance, params is not an array {:#?}",
               payload.params
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
   };

   let address_str = match params_array.get(0) {
      Some(Value::String(address)) => address,
      _ => {
         return {
            error!(
               "Invalid params for eth_getBalance, address is not a string {:#?}",
               params_array
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
   };

   let address = match Address::from_str(address_str) {
      Ok(address) => address,
      Err(_) => {
         return {
            error!(
               "Invalid params for eth_getBalance, address is not a valid address {:#?}",
               address_str
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
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

   let result = hex::encode(output);

   let response = JsonRpcResponse {
      jsonrpc: "2.0".to_string(),
      id: payload.id,
      result: Some(json!(result)),
      error: None,
   };

   Ok(response)
}

fn get_gas_price(ctx: ZeusCtx, payload: JsonRpcRequest) -> Result<JsonRpcResponse, Infallible> {
   let gas_price = ctx.get_base_fee(ctx.chain().id()).unwrap_or_default();

   let response = JsonRpcResponse {
      jsonrpc: "2.0".to_string(),
      id: payload.id,
      result: Some(json!(gas_price.next)),
      error: None,
   };

   Ok(response)
}

async fn estimate_gas(
   ctx: ZeusCtx,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let params_array = match payload.params {
      Value::Array(params) => params,
      _ => {
         return {
            error!(
               "Invalid params for eth_estimateGas, params is not an array {:#?}",
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
               "Invalid params for eth_estimateGas, params[0] is not an object {:#?}",
               params_array
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
   };

   let (calldata_str, from_str, to_str, value_str) = match (
      params_object.get("data"),
      params_object.get("from"),
      params_object.get("to"),
      params_object.get("value"),
   ) {
      (
         Some(Value::String(data)),
         Some(Value::String(from)),
         Some(Value::String(to)),
         Some(Value::String(value)),
      ) => (data, from, to, value),
      _ => {
         return {
            error!(
               "Invalid params for eth_estimateGas, from, to, and data are not strings {:#?}",
               params_object
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
   };

   let from = match Address::from_str(from_str) {
      Ok(from) => from,
      Err(_) => {
         return {
            error!(
               "Invalid params for eth_estimateGas, String is not a valid ethereum address {:#?}",
               from_str
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
               "Invalid params for eth_estimateGas, String is not a valid ethereum address {:#?}",
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
               "Invalid params for eth_estimateGas, String is not valid bytes {:#?}",
               calldata_str
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
   };

   let value = match U256::from_str(value_str) {
      Ok(value) => value,
      Err(_) => {
         return {
            error!(
               "Invalid params for eth_estimateGas, String is not a valid U256 value {:#?}",
               value_str
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
   };

   let chain = ctx.chain().id();
   let client = ctx.get_client_with_id(chain).unwrap();
   let tx = TransactionRequest::default()
      .with_from(from)
      .with_to(to)
      .with_input(calldata)
      .with_value(value);

   let gas = match client.estimate_gas(tx).await {
      Ok(output) => output,
      Err(e) => {
         return {
            error!("Error estimating gas: {:?}", e);
            Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id))
         };
      }
   };

   let response = JsonRpcResponse {
      jsonrpc: "2.0".to_string(),
      id: payload.id,
      result: Some(json!(gas)),
      error: None,
   };

   Ok(response)
}

fn switch_ethereum_chain(
   ctx: ZeusCtx,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let params_array = match payload.params {
      Value::Array(params) => params,
      _ => {
         return {
            error!(
               "Invalid params for wallet_switchEthereumChain, params is not an array {:#?}",
               payload.params
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
   };

   let object = match params_array.get(0) {
      Some(Value::Object(params)) => params,
      _ => {
         return {
            error!(
               "Invalid params for wallet_switchEthereumChain, params[0] is not an object {:#?}",
               params_array
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
   };

   let chain_id_hex_str = match object.get("chainId") {
      Some(Value::String(s)) => s,
      _ => {
         error!(
            "Invalid params for wallet_switchEthereumChain: Missing or invalid 'chainId' field (must be string), got {:?}",
            object
         );
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
      }
   };

   let chain_id = match chain_id_hex_str.strip_prefix("0x") {
      Some(hex_val) => match u64::from_str_radix(hex_val, 16) {
         Ok(id) => id,
         Err(e) => {
            error!("Failed to parse chainId hex '{}': {}", hex_val, e);
            return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
         }
      },
      None => {
         error!(
            "Invalid chainId format: Missing '0x' prefix in '{}'",
            chain_id_hex_str
         );
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
      }
   };

   let chain = match ChainId::new(chain_id) {
      Ok(chain) => chain,
      Err(_) => {
         return {
            error!(
               "Invalid params for wallet_switchEthereumChain, chain_id is not a valid chain id {:#?}",
               chain_id
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
   };

   ctx.write(|ctx| {
      ctx.chain = chain;
   });

   let response = JsonRpcResponse {
      jsonrpc: "2.0".to_string(),
      id: payload.id,
      result: Some(Value::Null),
      error: None,
   };

   Ok(response)
}

async fn eth_send_transaction(
   ctx: ZeusCtx,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let params_array = match payload.params {
      Value::Array(params) => params,
      _ => {
         return {
            error!(
               "Invalid params for eth_sendTransaction, params is not an array {:#?}",
               payload.params
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
   };

   // info!("eth_sendTransaction params: {:?}", params_array);

   let object = match params_array.get(0) {
      Some(Value::Object(params)) => params,
      _ => {
         return {
            error!(
               "Invalid params for eth_sendTransaction, params[0] is not an object {:#?}",
               params_array
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
   };

   let (data_str, from_str, _gas_hex, to_str, value_hex) = match (
      object.get("data"),
      object.get("from"),
      object.get("gas"),
      object.get("to"),
      object.get("value"),
   ) {
      (
         Some(Value::String(data)),
         Some(Value::String(from)),
         Some(Value::String(gas)),
         Some(Value::String(to)),
         Some(Value::String(value)),
      ) => (data, from, gas, to, value),
      _ => {
         return {
            error!(
               "Invalid params for eth_sendTransaction, from, to, gas, data, and value are not strings {:#?}",
               object
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
   };

   let call_data = match Bytes::from_str(data_str) {
      Ok(data) => data,
      Err(_) => {
         return {
            error!(
               "Invalid params for eth_sendTransaction, String is not valid bytes {:#?}",
               data_str
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
   };

   let from = match Address::from_str(from_str) {
      Ok(from) => from,
      Err(_) => {
         return {
            error!(
               "Invalid params for eth_sendTransaction, String is not a valid ethereum address {:#?}",
               from_str
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
               "Invalid params for eth_sendTransaction, String is not a valid ethereum address {:#?}",
               to_str
            );
            Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
         };
      }
   };

   let value = match value_hex.strip_prefix("0x") {
      Some(hex_val) => match U256::from_str_radix(hex_val, 16) {
         Ok(value) => value,
         Err(_) => {
            return {
               error!(
                  "Invalid params for eth_sendTransaction, String is not a valid U256 value {:#?}",
                  value_hex
               );
               Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
            };
         }
      },
      None => {
         error!(
            "Invalid params for eth_sendTransaction, String is not a valid U256 value {:#?}",
            value_hex
         );
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
      }
   };

   SHARED_GUI.write(|gui| {
      gui.tx_confirm_window.open();
      gui.tx_confirm_window.simulating();
   });

   let chain = ctx.chain().id();
   let client = ctx.get_client_with_id(chain).unwrap();
   let base_fee_fut = update::get_base_fee(ctx.clone(), chain);
   let bytecode_fut = client.get_code_at(to).into_future();

   let block = match client.get_block(BlockId::latest()).await {
      Ok(block) => block,
      Err(e) => {
         error!("Error getting latest block: {:?}", e);
         return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
      }
   };

   let factory = ForkFactory::new_sandbox_factory(client.clone(), None, None);
   let fork_db = factory.new_sandbox_fork();
   let mut evm = new_evm(chain, block, fork_db);

   evm.tx.caller = from;
   evm.tx.kind = TxKind::Call(to);
   evm.tx.data = call_data.clone();
   evm.tx.value = value;

   let sim_res = match evm.transact_commit(evm.tx.clone()) {
      Ok(res) => res,
      Err(e) => {
         error!("Error simulating tx: {:?}", e);
         return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
      }
   };

   let output = sim_res.output().unwrap_or_default();

   if !sim_res.is_success() {
      let err = revert_msg(&output);
      error!("Simulation failed: {}", err);
      return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
   }

   let gas_used = sim_res.gas_used();
   let logs = sim_res.into_logs();
   let balance_before = ctx.get_eth_balance(chain, from).unwrap_or_default();
   let state = evm.balance(from);
   let balance_after = if let Some(state) = state {
      NumericValue::format_wei(state.data, 18).wei2()
   } else {
      NumericValue::default().wei().unwrap_or_default()
   };

   let eth_spent = balance_before.wei2().checked_sub(balance_after);
   if eth_spent.is_none() {
      error!(
         "Error calculating eth spent, overflow occured, balance_before: {}, balance_after: {}",
         balance_before.wei2(),
         balance_after
      );
      return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
   }

   let bytecode = match bytecode_fut.await {
      Ok(bytecode) => bytecode,
      Err(e) => {
         error!("Error getting bytecode: {:?}", e);
         return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
      }
   };

   let native_currency = NativeCurrency::from_chain_id(chain).unwrap();
   let dapp = payload.origin.clone();
   let chain_id = ChainId::new(chain).unwrap();
   let eth_spent = NumericValue::format_wei(eth_spent.unwrap(), 18);
   let eth_price = ctx.get_currency_price(&Currency::from(native_currency.clone()));
   let eth_spent_value = NumericValue::value(eth_spent.f64(), eth_price.f64());
   let interact_to = to;
   let contract_interact = bytecode.len() > 0;
   let action = OnChainAction::new(ctx.clone(), chain, from, to, call_data.clone(), value, logs).await;
   let priority_fee = ctx.get_priority_fee(chain).unwrap_or_default();

   SHARED_GUI.write(|gui| {
      gui.tx_confirm_window.done_simulating();
      gui.tx_confirm_window.open_with(
         dapp,
         chain_id,
         true,
         eth_spent,
         eth_spent_value,
         NumericValue::default(),
         NumericValue::default(),
         gas_used,
         from,
         interact_to,
         contract_interact,
         action,
         priority_fee.formatted().clone(),
      );
   });

   // wait for the user to confirm or reject the transaction
   let mut confirmed = None;
   loop {
      tokio::time::sleep(std::time::Duration::from_millis(100)).await;

      SHARED_GUI.read(|gui| {
         confirmed = gui.tx_confirm_window.get_confirm();
      });

      if confirmed.is_some() {
         SHARED_GUI.write(|gui| {
            gui.tx_confirm_window.reset();
         });
         break;
      }
   }

   let confirmed = confirmed.unwrap();
   if !confirmed {
      return Ok(JsonRpcResponse::error(
         USER_REJECTED_REQUEST,
         payload.id,
      ));
   }
   let fee = SHARED_GUI.read(|gui| gui.tx_confirm_window.get_priority_fee());
   let priority_fee = if fee.is_zero() {
      ctx.get_priority_fee(chain).unwrap_or_default()
   } else {
      fee
   };

   let tx_method = tx::TxMethod::Other;
   if !ctx.wallet_exists(from) {
      return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
   }

   let base_fee = match base_fee_fut.await {
      Ok(base_fee) => base_fee,
      Err(e) => {
         error!("Error getting base fee: {:?}", e);
         return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
      }
   };

   let signer = ctx.get_wallet(from).key;

   let tx_params = tx::TxParams::new(
      tx_method,
      signer,
      to,
      value,
      chain_id,
      priority_fee.wei2(),
      base_fee.next,
      call_data,
      gas_used,
   );

   let res = tx_params.sufficient_balance(balance_before);
   if let Err(e) = res {
      SHARED_GUI.write(|gui| {
         gui.msg_window
            .open("Insufficient Balance".to_string(), e.to_string());
      });
      return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
   }

   let client = if chain_id.is_ethereum() {
      ctx.get_flashbots_fast_client().unwrap()
   } else {
      ctx.get_client_with_id(chain_id.id()).unwrap()
   };

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Sending Transaction...");
   });

   let _receipt = match tx::send_tx(client, tx_params).await {
      Ok(receipt) => receipt,
      Err(e) => {
         error!("Error sending tx: {:?}", e);
         SHARED_GUI.write(|gui| {
            gui.loading_window.reset();
            gui.msg_window
               .open("Error Sending Transaction", e.to_string());
         });
         return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
      }
   };

   SHARED_GUI.write(|gui| {
      gui.loading_window.reset();
      gui.msg_window.open("Transaction Sent", "");
   });

   Ok(JsonRpcResponse::ok(None, payload.id))
}

async fn handle_request(
   ctx: ZeusCtx,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   // info!("Request received from dapp {}", payload.origin);
   let method = payload.method.as_str();
   let res_body = if method == RequestMethod::RequestAccounts.as_str()
      || method == RequestMethod::EthAccounts.as_str()
   {
      request_accounts(ctx, payload)?
   } else if payload.method.as_str() == RequestMethod::ChainId.as_str() {
      chain_id(ctx, payload)?
   } else if payload.method.as_str() == RequestMethod::BlockNumber.as_str() {
      block_number(ctx, payload).await?
   } else if method == RequestMethod::GetBalance.as_str() {
      get_balance(ctx, payload)?
   } else if method == RequestMethod::EthCall.as_str() {
      eth_call(ctx, payload).await?
   } else if method == RequestMethod::EthGasPrice.as_str() {
      get_gas_price(ctx, payload)?
   } else if method == RequestMethod::EstimateGas.as_str() {
      estimate_gas(ctx, payload).await?
   } else if method == RequestMethod::WallletSwitchEthereumChain.as_str() {
      switch_ethereum_chain(ctx, payload)?
   } else if method == RequestMethod::SendTransaction.as_str() {
      eth_send_transaction(ctx, payload).await?
   } else {
      // TODO
      error!("Method '{}' not supported.", payload.method);
      JsonRpcResponse::error(UNSUPPORTED_METHOD, payload.id)
   };
   Ok(res_body)
}

// Handler for POST /api (JSON-RPC)
async fn api_handler(
   ctx: ZeusCtx,
   payload: JsonRpcRequest,
) -> Result<impl warp::Reply, Infallible> {
   // info!("POST /api requested with method: {}", payload.method);

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
