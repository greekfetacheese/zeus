use crate::connector::{
   ConnectorSession, ORIGIN_HEADER, TOKEN_HEADER, connector_session_path, generate_pairing_token,
   parse_dapp_origin, register_native_host, token_matches, write_connector_session,
};
use crate::core::{ZeusCtx, send_transaction, sign_message};
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::convert::Infallible;
use std::net::SocketAddr;
use tracing::{error, info, warn};
use warp::{Filter, Rejection, http::StatusCode};

use std::str::FromStr;

use zeus_eth::{
   alloy_network::TransactionBuilder,
   alloy_primitives::{Address, Bytes, TxHash, U256, hex},
   alloy_provider::Provider,
   alloy_rpc_types::{BlockId, TransactionRequest},
   currency::ERC20Token,
   types::ChainId,
};

/// Default connector port. If it is taken, [`bind_connector_listener`] walks
/// nearby high ports and the bound port is written to `connector.json`.
pub const SERVER_PORT: u16 = 65534;
/// Preferred port plus this many decrements (skipping 0).
const CONNECTOR_PORT_ATTEMPTS: u16 = 32;

fn connector_port_candidates(preferred: u16) -> impl Iterator<Item = u16> {
   (0..CONNECTOR_PORT_ATTEMPTS)
      .filter_map(move |i| preferred.checked_sub(i))
      .filter(|&p| p != 0)
}

/// Bind `127.0.0.1` on `preferred`, then nearby ports if that one is in use.
async fn bind_connector_listener(
   preferred: u16,
) -> Result<(tokio::net::TcpListener, u16), std::io::Error> {
   let mut last_err = None;
   for port in connector_port_candidates(preferred) {
      let addr = SocketAddr::from(([127, 0, 0, 1], port));
      match tokio::net::TcpListener::bind(addr).await {
         Ok(listener) => {
            if port != preferred {
               warn!(
                  "Connector port {} is in use, listening on {}",
                  preferred, port
               );
            }
            return Ok((listener, port));
         }
         Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            last_err = Some(e);
         }
         Err(e) => return Err(e),
      }
   }

   Err(last_err.unwrap_or_else(|| {
      std::io::Error::new(
         std::io::ErrorKind::AddrInUse,
         "no free connector port on 127.0.0.1",
      )
   }))
}

// EIP-1193 Error codes
pub const USER_REJECTED_REQUEST: i32 = 4001;
pub const UNAUTHORIZED: i32 = 4100;
pub const UNSUPPORTED_METHOD: i32 = 4200;
pub const DISCONNECTED: i32 = 4900;
pub const CHAIN_DISCONNECTED: i32 = 4901;
/// EIP-1193 / MetaMask: unknown chain on wallet_switchEthereumChain
pub const UNRECOGNIZED_CHAIN: i32 = 4902;

// JSON-RPC Error Codes
pub const INVALID_PARAMS: i32 = -32602;
pub const INTERNAL_ERROR: i32 = -32603;

/// Type of a request we expect to receive from the extension/dapp
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequestMethod {
   WalletAddEthereumChain,
   WalletSwitchEthereumChain,
   WalletGetPermissions,
   WalletGetCapabilities,
   WalletRequestPermissions,
   WalletRevokePermissions,
   EthGetTransactionByHash,
   EthGetTransactionReceipt,
   EthGetBlockByNumber,
   EthAccounts,
   RequestAccounts,
   EthSendTransaction,
   BlockNumber,
   EthCall,
   EthGetCode,
   EthGetStorageAt,
   ChainId,
   EstimateGas,
   EthGasPrice,
   EthMaxPriorityFeePerGas,
   GetBalance,
   EthSignedTypedDataV4,
   PersonalSign,
}

impl RequestMethod {
   pub fn from_str(s: &str) -> Result<Self, anyhow::Error> {
      match s {
         "wallet_addEthereumChain" => Ok(RequestMethod::WalletAddEthereumChain),
         "wallet_switchEthereumChain" => Ok(RequestMethod::WalletSwitchEthereumChain),
         "wallet_getPermissions" => Ok(RequestMethod::WalletGetPermissions),
         "wallet_getCapabilities" => Ok(RequestMethod::WalletGetCapabilities),
         "wallet_requestPermissions" => Ok(RequestMethod::WalletRequestPermissions),
         "wallet_revokePermissions" => Ok(RequestMethod::WalletRevokePermissions),
         "eth_getTransactionByHash" => Ok(RequestMethod::EthGetTransactionByHash),
         "eth_getTransactionReceipt" => Ok(RequestMethod::EthGetTransactionReceipt),
         "eth_getBlockByNumber" => Ok(RequestMethod::EthGetBlockByNumber),
         "eth_accounts" => Ok(RequestMethod::EthAccounts),
         "eth_requestAccounts" => Ok(RequestMethod::RequestAccounts),
         "eth_sendTransaction" => Ok(RequestMethod::EthSendTransaction),
         "eth_blockNumber" => Ok(RequestMethod::BlockNumber),
         "eth_call" => Ok(RequestMethod::EthCall),
         "eth_getCode" => Ok(RequestMethod::EthGetCode),
         "eth_getStorageAt" => Ok(RequestMethod::EthGetStorageAt),
         "eth_chainId" => Ok(RequestMethod::ChainId),
         "eth_estimateGas" => Ok(RequestMethod::EstimateGas),
         "eth_gasPrice" => Ok(RequestMethod::EthGasPrice),
         "eth_maxPriorityFeePerGas" => Ok(RequestMethod::EthMaxPriorityFeePerGas),
         "eth_getBalance" => Ok(RequestMethod::GetBalance),
         "eth_signTypedData_v4" => Ok(RequestMethod::EthSignedTypedDataV4),
         "personal_sign" => Ok(RequestMethod::PersonalSign),
         _ => Err(anyhow!("Invalid Request Method: {:?}", s)),
      }
   }

   pub fn as_str(&self) -> &'static str {
      match self {
         RequestMethod::WalletAddEthereumChain => "wallet_addEthereumChain",
         RequestMethod::WalletSwitchEthereumChain => "wallet_switchEthereumChain",
         RequestMethod::WalletGetPermissions => "wallet_getPermissions",
         RequestMethod::WalletGetCapabilities => "wallet_getCapabilities",
         RequestMethod::WalletRequestPermissions => "wallet_requestPermissions",
         RequestMethod::WalletRevokePermissions => "wallet_revokePermissions",
         RequestMethod::EthGetTransactionByHash => "eth_getTransactionByHash",
         RequestMethod::EthGetTransactionReceipt => "eth_getTransactionReceipt",
         RequestMethod::EthGetBlockByNumber => "eth_getBlockByNumber",
         RequestMethod::EthAccounts => "eth_accounts",
         RequestMethod::RequestAccounts => "eth_requestAccounts",
         RequestMethod::EthSendTransaction => "eth_sendTransaction",
         RequestMethod::BlockNumber => "eth_blockNumber",
         RequestMethod::EthCall => "eth_call",
         RequestMethod::EthGetCode => "eth_getCode",
         RequestMethod::EthGetStorageAt => "eth_getStorageAt",
         RequestMethod::ChainId => "eth_chainId",
         RequestMethod::EstimateGas => "eth_estimateGas",
         RequestMethod::EthGasPrice => "eth_gasPrice",
         RequestMethod::EthMaxPriorityFeePerGas => "eth_maxPriorityFeePerGas",
         RequestMethod::GetBalance => "eth_getBalance",
         RequestMethod::EthSignedTypedDataV4 => "eth_signTypedData_v4",
         RequestMethod::PersonalSign => "personal_sign",
      }
   }
}

#[derive(Deserialize, Debug)]
struct ApiRequestBody {
   /// Ignored if present. Origin comes from `X-Zeus-Origin` (extension tab URL).
   #[serde(default, rename = "origin")]
   _origin: Option<String>,
   #[serde(flatten)]
   rpc_request: JsonRpcRequest,
}

#[derive(Deserialize, Debug)]
/// Request received from the extension
struct JsonRpcRequest {
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
   pub fn error_res(error: JsonRpcError, id: Value) -> Self {
      Self {
         jsonrpc: "2.0".to_string(),
         id,
         result: None,
         error: Some(error),
      }
   }

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
   pub fn new(code: i32, err: String, data: Option<Value>) -> Self {
      Self {
         code,
         message: err,
         data,
      }
   }

   pub fn from(code: i32) -> Self {
      match code {
         USER_REJECTED_REQUEST => Self::user_rejected_request(),
         UNAUTHORIZED => Self::unauthorized(),
         UNSUPPORTED_METHOD => Self::unsupported_method(),
         DISCONNECTED => Self::disconnected(),
         CHAIN_DISCONNECTED => Self::chain_disconnected(),
         UNRECOGNIZED_CHAIN => Self::unrecognized_chain(),
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

   pub fn unrecognized_chain() -> Self {
      Self {
         code: UNRECOGNIZED_CHAIN,
         message:
            "Unrecognized chain ID. Try adding the chain using wallet_addEthereumChain first."
               .to_string(),
         data: None,
      }
   }
}

/// True when the user declined a confirm/sign prompt.
///
/// Dapps (OpenSea/viem, etc.) retry `-32603` up to 3 times, which re-opens
/// the confirm UI. EIP-1193 `4001` is not retried.
fn is_user_rejected(err: &anyhow::Error) -> bool {
   let msg = err.to_string();
   msg.contains("Transaction rejected") || msg.contains("You cancelled the signing process")
}

/// JSON-RPC QUANTITY: `0x` + unpadded hex (zero is `0x0`).
fn hex_quantity_u64(n: u64) -> String {
   format!("0x{:x}", n)
}

fn hex_quantity_u256(n: U256) -> String {
   format!("0x{:x}", n)
}

fn hex_data(bytes: &[u8]) -> String {
   format!("0x{}", hex::encode(bytes))
}

fn parse_hex_chain_id(chain_id_hex_str: &str) -> Option<u64> {
   let hex_val = chain_id_hex_str
      .strip_prefix("0x")
      .or_else(|| chain_id_hex_str.strip_prefix("0X"))?;
   u64::from_str_radix(hex_val, 16).ok()
}

fn rpc_opt_string<'a>(object: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
   match object.get(key) {
      Some(Value::String(s)) => Some(s.as_str()),
      _ => None,
   }
}

fn parse_rpc_u256(value: Option<&Value>) -> Result<U256, ()> {
   match value {
      None => Ok(U256::ZERO),
      Some(Value::String(s)) => {
         if s.is_empty() {
            return Ok(U256::ZERO);
         }
         if let Some(hex_val) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            if hex_val.is_empty() {
               return Ok(U256::ZERO);
            }
            U256::from_str_radix(hex_val, 16).map_err(|_| ())
         } else {
            U256::from_str_radix(s, 10).map_err(|_| ())
         }
      }
      Some(Value::Number(n)) => Ok(n.as_u64().map_or(U256::ZERO, U256::from)),
      _ => Err(()),
   }
}

fn parse_rpc_bytes(value: Option<&Value>) -> Result<Bytes, ()> {
   match value {
      None => Ok(Bytes::new()),
      Some(Value::String(s)) if s.is_empty() || s == "0x" || s == "0X" => Ok(Bytes::new()),
      Some(Value::String(s)) => Bytes::from_str(s).map_err(|_| ()),
      _ => Err(()),
   }
}

fn rpc_params_array<'a>(
   params: &'a Value,
   method: &str,
   min_len: usize,
) -> Result<&'a [Value], ()> {
   let Value::Array(arr) = params else {
      error!(
         "Invalid params for {}, params is not an array {:#?}",
         method, params
      );
      return Err(());
   };
   if arr.len() < min_len {
      error!(
         "Invalid params for {}: expected at least {} elements, got {}",
         method,
         min_len,
         arr.len()
      );
      return Err(());
   }
   Ok(arr)
}

fn rpc_params_object<'a>(
   params: &'a Value,
   method: &str,
) -> Result<&'a serde_json::Map<String, Value>, ()> {
   let arr = rpc_params_array(params, method, 1)?;
   match arr.first() {
      Some(Value::Object(obj)) => Ok(obj),
      _ => {
         error!(
            "Invalid params for {}, params[0] is not an object {:#?}",
            method, arr
         );
         Err(())
      }
   }
}

fn parse_rpc_address(s: &str, method: &str) -> Result<Address, ()> {
   Address::from_str(s).map_err(|_| {
      error!(
         "Invalid params for {}, String is not a valid ethereum address {:#?}",
         method, s
      );
   })
}

fn rpc_param_address(arr: &[Value], index: usize, method: &str) -> Result<Address, ()> {
   let Some(Value::String(s)) = arr.get(index) else {
      error!(
         "Invalid params for {}: params[{}] is not a string",
         method, index
      );
      return Err(());
   };
   parse_rpc_address(s, method)
}

fn parse_rpc_block_id(value: Option<&Value>, method: &str) -> Result<BlockId, ()> {
   match value {
      None => Ok(BlockId::latest()),
      Some(Value::String(s)) => BlockId::from_str(s).map_err(|e| {
         error!("Invalid params for {}: {}: {}", method, s, e);
      }),
      Some(Value::Number(n)) => {
         let Some(num) = n.as_u64() else {
            error!(
               "Invalid params for {}: block number overflow",
               method
            );
            return Err(());
         };
         BlockId::from_str(&hex_quantity_u64(num)).map_err(|_| {
            error!(
               "Invalid params for {}: invalid block number",
               method
            );
         })
      }
      _ => {
         error!(
            "Invalid params for {}: block id is not a tag/number",
            method
         );
         Err(())
      }
   }
}

struct RpcTxCall {
   from: Address,
   to: Address,
   data: Bytes,
   value: U256,
}

fn parse_rpc_tx_call(
   object: &serde_json::Map<String, Value>,
   default_from: Address,
   method: &str,
) -> Result<RpcTxCall, ()> {
   let Some(to_str) = rpc_opt_string(object, "to") else {
      error!(
         "Invalid params for {}, missing 'to' {:#?}",
         method, object
      );
      return Err(());
   };
   let to = parse_rpc_address(to_str, method)?;

   let from = match rpc_opt_string(object, "from") {
      Some(from_str) => parse_rpc_address(from_str, method)?,
      None => default_from,
   };

   let data_val = object.get("data").or_else(|| object.get("input"));
   let data = match parse_rpc_bytes(data_val) {
      Ok(data) => data,
      Err(_) => {
         error!(
            "Invalid params for {}, data/input is not valid bytes {:#?}",
            method, data_val
         );
         return Err(());
      }
   };

   let value = match parse_rpc_u256(object.get("value")) {
      Ok(value) => value,
      Err(_) => {
         error!(
            "Invalid params for {}, value is not a valid U256 {:#?}",
            method,
            object.get("value")
         );
         return Err(());
      }
   };

   Ok(RpcTxCall {
      from,
      to,
      data,
      value,
   })
}

impl RpcTxCall {
   fn into_tx(self) -> TransactionRequest {
      TransactionRequest::default()
         .with_from(self.from)
         .with_to(self.to)
         .with_input(self.data)
         .with_value(self.value)
   }
}

async fn wait_for_user_confirm() -> bool {
   loop {
      tokio::time::sleep(std::time::Duration::from_millis(100)).await;
      let confirmed = SHARED_GUI.read(|gui| gui.confirm_window.get_confirm());
      if let Some(confirmed) = confirmed {
         SHARED_GUI.write(|gui| {
            gui.confirm_window.reset();
         });
         return confirmed;
      }
   }
}

// Handler for GET /status
async fn status_handler(ctx: ZeusCtx) -> Result<impl warp::Reply, Infallible> {
   let chain = ctx.chain().id_as_hex();
   let accounts = vec![ctx.current_wallet_info().address.to_string()];
   let connected_origins = ctx.get_connected_dapps();

   let res = json!({
       "status": true,
       "accounts": accounts,
       "chainId": chain,
       "connectedOrigins": connected_origins,
   });

   Ok(warp::reply::json(&res))
}

fn request_accounts(
   ctx: ZeusCtx,
   origin: &str,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let current_wallet = ctx.current_wallet_info().address;
   let result = if ctx.is_dapp_connected(origin) {
      json!(vec![current_wallet.to_string()])
   } else {
      json!([])
   };
   Ok(JsonRpcResponse::ok(Some(result), payload.id))
}

fn get_permissions(
   ctx: ZeusCtx,
   origin: &str,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let current_wallet = ctx.current_wallet_info().address.to_string();
   let result = if ctx.is_dapp_connected(origin) {
      json!([{
          "parentCapability": "eth_accounts",
          "caveats": [{
              "type": "restrictReturnedAccounts",
              "value": [current_wallet]
          }]
      }])
   } else {
      json!([])
   };
   Ok(JsonRpcResponse::ok(Some(result), payload.id))
}

/// EIP-5792. Zeus does not implement `wallet_sendCalls` / atomic batches /
/// paymaster, so this is an empty capabilities map (not `-32602`).
fn get_capabilities(payload: JsonRpcRequest) -> Result<JsonRpcResponse, Infallible> {
   match &payload.params {
      Value::Null | Value::Array(_) => {}
      _ => return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id)),
   }
   Ok(JsonRpcResponse::ok(Some(json!({})), payload.id))
}

/// Aka disconnect
fn wallet_revoke_permissions(
   ctx: ZeusCtx,
   origin: String,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   ctx.disconnect_dapp(&origin);
   Ok(JsonRpcResponse::ok(Some(Value::Null), payload.id))
}

/// Depending on the dapp, we may receive eth_requestAccounts or wallet_getPermissions
/// as the request method for connection
async fn connect(
   ctx: ZeusCtx,
   origin: String,
   payload: JsonRpcRequest,
   method: RequestMethod,
) -> Result<JsonRpcResponse, Infallible> {
   SHARED_GUI.write(|gui| {
      gui.confirm_window.open("Connect to Dapp");
      gui.confirm_window.set_msg2(origin.clone());
      gui.request_repaint();
   });

   if !wait_for_user_confirm().await {
      return Ok(JsonRpcResponse::error(
         USER_REJECTED_REQUEST,
         payload.id,
      ));
   }

   ctx.connect_dapp(origin.clone());

   let current_wallet = ctx.current_wallet_info().address.to_string();

   let result = match method {
      RequestMethod::RequestAccounts => Some(json!(vec![current_wallet])),
      RequestMethod::WalletRequestPermissions => Some(json!([{
          "parentCapability": "eth_accounts",
          "caveats": [{
              "type": "restrictReturnedAccounts",
              "value": [current_wallet]
          }]
      }])),
      _ => Some(json!([])),
   };

   Ok(JsonRpcResponse::ok(result, payload.id))
}

fn chain_id(ctx: ZeusCtx, payload: JsonRpcRequest) -> Result<JsonRpcResponse, Infallible> {
   let chain_id = ctx.chain().id_as_hex();
   Ok(JsonRpcResponse::ok(
      Some(json!(chain_id)),
      payload.id,
   ))
}

async fn block_number(
   ctx: ZeusCtx,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let block = match ctx.get_latest_block().await {
      Ok(block) => block,
      Err(e) => {
         error!("Error getting latest block: {:?}", e);
         return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
      }
   };

   let response = JsonRpcResponse {
      jsonrpc: "2.0".to_string(),
      id: payload.id,
      result: Some(json!(hex_quantity_u64(block.number))),
      error: None,
   };

   Ok(response)
}

fn get_balance(ctx: ZeusCtx, payload: JsonRpcRequest) -> Result<JsonRpcResponse, Infallible> {
   let arr = match rpc_params_array(&payload.params, "eth_getBalance", 1) {
      Ok(arr) => arr,
      Err(()) => return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id)),
   };

   let address = match rpc_param_address(arr, 0, "eth_getBalance") {
      Ok(address) => address,
      Err(()) => return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id)),
   };

   let chain = ctx.chain().id();
   let balance = ctx.get_eth_balance(chain, address);

   Ok(JsonRpcResponse::ok(
      Some(json!(hex_quantity_u256(balance.wei()))),
      payload.id,
   ))
}

async fn eth_get_storage_at(
   ctx: ZeusCtx,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let array = match rpc_params_array(&payload.params, "eth_getStorageAt", 2) {
      Ok(arr) => arr,
      Err(()) => return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id)),
   };

   let address = match rpc_param_address(array, 0, "eth_getStorageAt") {
      Ok(address) => address,
      Err(()) => return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id)),
   };

   let slot = match parse_rpc_u256(array.get(1)) {
      Ok(slot) => slot,
      Err(()) => {
         error!(
            "Invalid params for eth_getStorageAt: slot is not a valid U256 {:#?}",
            array.get(1)
         );
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
      }
   };

   let block = match parse_rpc_block_id(array.get(2), "eth_getStorageAt") {
      Ok(block) => block,
      Err(()) => return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id)),
   };

   let storage = match ctx.get_storage(block, address, slot).await {
      Ok(storage) => storage,
      Err(_) => return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id)),
   };

   Ok(JsonRpcResponse::ok(
      Some(Value::String(hex_data(
         &storage.to_be_bytes_vec(),
      ))),
      payload.id,
   ))
}

async fn eth_get_code(
   ctx: ZeusCtx,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let array = match rpc_params_array(&payload.params, "eth_getCode", 1) {
      Ok(arr) => arr,
      Err(()) => return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id)),
   };

   let address = match rpc_param_address(array, 0, "eth_getCode") {
      Ok(address) => address,
      Err(()) => return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id)),
   };

   let block = match parse_rpc_block_id(array.get(1), "eth_getCode") {
      Ok(block) => block,
      Err(()) => return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id)),
   };

   let code = match ctx.get_code(block, address).await {
      Ok(code) => code,
      Err(e) => {
         let err = JsonRpcError::new(INTERNAL_ERROR, e.to_string(), None);
         return Ok(JsonRpcResponse::error_res(err, payload.id));
      }
   };

   Ok(JsonRpcResponse::ok(
      Some(Value::String(hex_data(&code))),
      payload.id,
   ))
}

async fn eth_get_transaction_by_hash(
   ctx: ZeusCtx,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let hash = match payload.params {
      Value::Array(arr) if arr.len() == 1 => {
         let hash_str = match &arr[0] {
            Value::String(s) => s,
            _ => {
               error!("Invalid params for eth_getTransactionByHash: params[0] is not a string");
               return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
            }
         };
         match TxHash::from_str(hash_str) {
            Ok(hash) => hash,
            Err(e) => {
               error!("Invalid transaction hash: {:?} - {}", hash_str, e);
               return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
            }
         }
      }
      _ => {
         error!("Invalid params for eth_getTransactionByHash: expected array with 1 element");
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
      }
   };

   let tx = match ctx.get_tx_by_hash(hash).await {
      Ok(tx) => tx,
      Err(e) => {
         let err = JsonRpcError::new(INTERNAL_ERROR, e.to_string(), None);
         return Ok(JsonRpcResponse::error_res(err, payload.id));
      }
   };

   let result = match tx {
      Some(tx) => match serde_json::to_value(tx) {
         Ok(val) => Some(val),
         Err(e) => {
            error!("Error serializing transaction: {:?}", e);
            return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
         }
      },
      None => Some(Value::Null),
   };

   Ok(JsonRpcResponse::ok(result, payload.id))
}

async fn eth_get_transaction_receipt(
   ctx: ZeusCtx,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let hash = match payload.params {
      Value::Array(arr) if arr.len() == 1 => {
         let hash_str = match &arr[0] {
            Value::String(s) => s,
            _ => {
               error!("Invalid params for eth_getTransactionReceipt: params[0] is not a string");
               return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
            }
         };
         match TxHash::from_str(hash_str) {
            Ok(hash) => hash,
            Err(e) => {
               error!("Invalid transaction hash: {:?} - {}", hash_str, e);
               return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
            }
         }
      }
      _ => {
         error!("Invalid params for eth_getTransactionReceipt: expected array with 1 element");
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
      }
   };

   let receipt = match ctx.get_receipt_by_hash(hash).await {
      Ok(receipt) => receipt,
      Err(e) => {
         let err = JsonRpcError::new(INTERNAL_ERROR, e.to_string(), None);
         return Ok(JsonRpcResponse::error_res(err, payload.id));
      }
   };

   let result = match receipt {
      Some(receipt) => match serde_json::to_value(receipt) {
         Ok(val) => Some(val),
         Err(e) => {
            error!("Error serializing receipt: {:?}", e);
            return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
         }
      },
      None => Some(Value::Null),
   };

   Ok(JsonRpcResponse::ok(result, payload.id))
}

async fn eth_get_block_by_number(
   ctx: ZeusCtx,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let array = match payload.params {
      Value::Array(arr) if !arr.is_empty() => arr,
      _ => {
         error!("Invalid params for eth_getBlockByNumber: expected [blockNumber, hydrated]");
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
      }
   };

   let block_str = match &array[0] {
      Value::String(s) => s.clone(),
      Value::Number(n) => match n.as_u64() {
         Some(num) => hex_quantity_u64(num),
         None => {
            error!("Invalid params for eth_getBlockByNumber: block number overflow");
            return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
         }
      },
      _ => {
         error!("Invalid params for eth_getBlockByNumber: params[0] is not a block tag/number");
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
      }
   };

   let hydrated = match array.get(1) {
      Some(Value::Bool(b)) => *b,
      Some(Value::String(s)) => s.eq_ignore_ascii_case("true"),
      _ => false,
   };

   let block_id = match BlockId::from_str(&block_str) {
      Ok(id) => id,
      Err(e) => {
         error!(
            "Invalid params for eth_getBlockByNumber: {}: {}",
            block_str, e
         );
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
      }
   };

   let chain = ctx.chain().id();
   let client = ctx.get_zeus_client();
   let block = match client
      .request(chain, move |client| async move {
         let req = client.get_block(block_id);
         if hydrated {
            req.full().await.map_err(|e| anyhow!("{:?}", e))
         } else {
            req.await.map_err(|e| anyhow!("{:?}", e))
         }
      })
      .await
   {
      Ok(block) => block,
      Err(e) => {
         let err = JsonRpcError::new(INTERNAL_ERROR, e.to_string(), None);
         return Ok(JsonRpcResponse::error_res(err, payload.id));
      }
   };

   let result = match block {
      Some(block) => match serde_json::to_value(block) {
         Ok(val) => Some(val),
         Err(e) => {
            error!("Error serializing block: {:?}", e);
            return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
         }
      },
      None => Some(Value::Null),
   };

   Ok(JsonRpcResponse::ok(result, payload.id))
}

async fn eth_call(ctx: ZeusCtx, payload: JsonRpcRequest) -> Result<JsonRpcResponse, Infallible> {
   let params_object = match rpc_params_object(&payload.params, "eth_call") {
      Ok(object) => object,
      Err(()) => return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id)),
   };

   let call = match parse_rpc_tx_call(
      params_object,
      ctx.current_wallet_info().address,
      "eth_call",
   ) {
      Ok(call) => call,
      Err(()) => return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id)),
   };

   let output = match ctx.get_eth_call(call.into_tx()).await {
      Ok(output) => output,
      Err(e) => {
         let err = JsonRpcError::new(INTERNAL_ERROR, e.to_string(), None);
         return Ok(JsonRpcResponse::error_res(err, payload.id));
      }
   };

   Ok(JsonRpcResponse::ok(
      Some(json!(hex_data(&output.result))),
      payload.id,
   ))
}

fn get_gas_price(ctx: ZeusCtx, payload: JsonRpcRequest) -> Result<JsonRpcResponse, Infallible> {
   let gas_price = ctx.get_base_fee(ctx.chain().id()).unwrap_or_default();
   Ok(JsonRpcResponse::ok(
      Some(json!(hex_quantity_u64(gas_price.next))),
      payload.id,
   ))
}

async fn max_priority_fee_per_gas(
   ctx: ZeusCtx,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let chain = ctx.chain().id();
   if let Some(fee) = ctx.get_priority_fee(chain) {
      return Ok(JsonRpcResponse::ok(
         Some(json!(hex_quantity_u256(fee.wei()))),
         payload.id,
      ));
   }

   let client = ctx.get_zeus_client();
   let fee = match client
      .request(chain, |client| async move {
         client.get_max_priority_fee_per_gas().await.map_err(|e| anyhow!("{:?}", e))
      })
      .await
   {
      Ok(fee) => fee,
      Err(e) => {
         error!("Error getting max priority fee: {:?}", e);
         return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
      }
   };

   Ok(JsonRpcResponse::ok(
      Some(json!(hex_quantity_u256(U256::from(fee)))),
      payload.id,
   ))
}

async fn estimate_gas(
   ctx: ZeusCtx,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   #[cfg(feature = "dev")]
   info!(
      "Received estimateGas params {:#?}",
      payload.params
   );

   let object = match rpc_params_object(&payload.params, "eth_estimateGas") {
      Ok(object) => object,
      Err(()) => return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id)),
   };

   let call = match parse_rpc_tx_call(
      object,
      ctx.current_wallet_info().address,
      "eth_estimateGas",
   ) {
      Ok(call) => call,
      Err(()) => return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id)),
   };

   let tx = call.into_tx();

   match ctx.estimate_gas(tx).await {
      Ok(gas) => Ok(JsonRpcResponse::ok(
         Some(json!(hex_quantity_u64(gas))),
         payload.id,
      )),
      Err(e) => {
         let err = JsonRpcError::new(INTERNAL_ERROR, e.to_string(), None);
         Ok(JsonRpcResponse::error_res(err, payload.id))
      }
   }
}

async fn eth_sign_typed_data_v4(
   ctx: ZeusCtx,
   origin: String,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let typed_data_str = match payload.params.get(1) {
      Some(Value::String(s)) => s,
      _ => {
         error!("Invalid params for eth_signTypedData_v4: expected string at params[1]");
         return Ok(JsonRpcResponse::error(-32602, payload.id));
      }
   };

   let typed_data_value: Value = match serde_json::from_str(typed_data_str) {
      Ok(v) => v,
      Err(e) => {
         error!("Failed to parse typed data string: {:?}", e);
         return Ok(JsonRpcResponse::error(-32602, payload.id));
      }
   };

   let chain = ctx.chain();
   let signature = match sign_message(
      ctx,
      origin.clone(),
      chain,
      Some(typed_data_value),
      None,
      None,
   )
   .await
   {
      Ok(signature) => signature,
      Err(e) => {
         let rejected = is_user_rejected(&e);
         SHARED_GUI.write(|gui| {
            gui.loading_window.reset();
            if !rejected {
               let msg = format!("Error Signing Message: {}", e);
               gui.msg_window.open(msg);
            }
            gui.request_repaint();
         });
         if rejected {
            return Ok(JsonRpcResponse::error(
               USER_REJECTED_REQUEST,
               payload.id,
            ));
         }
         error!("Error signing message: {:?}", e);
         return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
      }
   };

   let sig_hex = hex_data(&signature.as_bytes());

   let response = JsonRpcResponse::ok(Some(Value::String(sig_hex)), payload.id);
   Ok(response)
}

async fn personal_sign(
   ctx: ZeusCtx,
   origin: String,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   // Validate params: array of exactly 2 elements - [message_hex: String, address: String]
   let params_array = match payload.params {
      Value::Array(params) if params.len() == 2 => params,
      _ => {
         error!(
            "Invalid params for personal_sign: expected array with 2 elements (message, address)"
         );
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
      }
   };

   let message_hex = match &params_array[0] {
      Value::String(s) => s.clone(),
      _ => {
         error!("Invalid params for personal_sign: message must be a hex string");
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
      }
   };

   let address_str = match &params_array[1] {
      Value::String(s) => s.clone(),
      _ => {
         error!("Invalid params for personal_sign: address must be a string");
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
      }
   };

   let address = match Address::from_str(&address_str) {
      Ok(addr) => addr,
      Err(e) => {
         error!("Invalid address for personal_sign: {}", e);
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
      }
   };

   // Ensure the address matches the current wallet
   let current_wallet = ctx.current_wallet_info().address;
   if address != current_wallet {
      error!(
         "personal_sign: Address mismatch - requested {} but current is {}",
         address, current_wallet
      );
      return Ok(JsonRpcResponse::error(UNAUTHORIZED, payload.id)); // Or a specific error like 4100
   }

   // Decode the hex message to bytes
   let message_bytes = match hex::decode(message_hex.strip_prefix("0x").unwrap_or(&message_hex)) {
      Ok(bytes) => bytes,
      Err(e) => {
         error!("Invalid hex message for personal_sign: {}", e);
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
      }
   };

   let full_message = String::from_utf8_lossy(&message_bytes).to_string();

   let chain = ctx.chain();
   let signature = match sign_message(ctx, origin, chain, None, Some(full_message), None).await {
      Ok(sig) => sig,
      Err(e) => {
         let rejected = is_user_rejected(&e);
         SHARED_GUI.write(|gui| {
            gui.loading_window.reset();
            if !rejected {
               let msg = format!("Error Signing Message: {}", e);
               gui.msg_window.open(msg);
            }
            gui.request_repaint();
         });
         if rejected {
            return Ok(JsonRpcResponse::error(
               USER_REJECTED_REQUEST,
               payload.id,
            ));
         }
         error!("Error signing personal message: {:?}", e);
         return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
      }
   };

   let sig_hex = hex_data(&signature.as_bytes());

   Ok(JsonRpcResponse::ok(
      Some(Value::String(sig_hex)),
      payload.id,
   ))
}

/// Zeus only knows a fixed chain set. If the requested chain is supported,
/// confirm and switch; otherwise 4902 (same as an unknown switch).
async fn switch_ethereum_chain(
   ctx: ZeusCtx,
   origin: String,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   match parse_requested_chain(&payload.params) {
      Ok(chain) => apply_chain_switch(ctx, origin, chain, payload.id).await,
      Err(code) => Ok(JsonRpcResponse::error(code, payload.id)),
   }
}

fn parse_requested_chain(params: &Value) -> Result<ChainId, i32> {
   let object = match rpc_params_object(params, "chain switch/add") {
      Ok(object) => object,
      Err(()) => return Err(INVALID_PARAMS),
   };

   let chain_id_hex_str = match object.get("chainId") {
      Some(Value::String(s)) => s,
      _ => {
         error!(
            "Invalid params for chain switch/add: Missing or invalid 'chainId' field (must be string), got {:?}",
            object
         );
         return Err(INVALID_PARAMS);
      }
   };

   let chain_id = match parse_hex_chain_id(chain_id_hex_str) {
      Some(id) => id,
      None => {
         error!(
            "Failed to parse chainId hex '{}'",
            chain_id_hex_str
         );
         return Err(INVALID_PARAMS);
      }
   };

   match ChainId::new(chain_id) {
      Ok(chain) => Ok(chain),
      Err(_) => {
         error!("Unrecognized chain id {:#?}", chain_id);
         Err(UNRECOGNIZED_CHAIN)
      }
   }
}

async fn apply_chain_switch(
   ctx: ZeusCtx,
   origin: String,
   chain: ChainId,
   payload_id: Value,
) -> Result<JsonRpcResponse, Infallible> {
   if ctx.chain() == chain {
      return Ok(JsonRpcResponse::ok(Some(Value::Null), payload_id));
   }

   SHARED_GUI.write(|gui| {
      gui.confirm_window.open("Switch Network");
      gui.confirm_window.set_msg2(format!(
         "{} wants to switch to {}",
         origin,
         chain.name()
      ));
      gui.request_repaint();
   });

   if !wait_for_user_confirm().await {
      return Ok(JsonRpcResponse::error(
         USER_REJECTED_REQUEST,
         payload_id,
      ));
   }

   ctx.write(|ctx| {
      ctx.chain = chain;
   });

   SHARED_GUI.write(|gui| {
      gui.header.set_current_chain(chain);
      gui.request_repaint();
   });

   Ok(JsonRpcResponse::ok(Some(Value::Null), payload_id))
}

async fn eth_send_transaction(
   ctx: ZeusCtx,
   origin: String,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let object = match rpc_params_object(&payload.params, "eth_sendTransaction") {
      Ok(object) => object,
      Err(()) => return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id)),
   };

   let RpcTxCall {
      from,
      to: transact_to,
      data: call_data,
      value,
   } = match parse_rpc_tx_call(
      object,
      ctx.current_wallet_info().address,
      "eth_sendTransaction",
   ) {
      Ok(call) => call,
      Err(()) => return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id)),
   };

   let chain = ctx.chain();

   let (receipt, tx_rich) = match send_transaction(
      ctx.clone(),
      origin,
      None,
      chain,
      true,
      from,
      transact_to,
      call_data,
      value,
      Vec::new(),
   )
   .await
   {
      Ok(res) => res,
      Err(e) => {
         let rejected = is_user_rejected(&e);
         SHARED_GUI.write(|gui| {
            gui.loading_window.reset();
            gui.notification.reset();
            gui.tx_confirmation_window.reset();
            if !rejected {
               let msg = format!("Error Sending Transaction: {}", e);
               gui.msg_window.open(msg);
            }
            gui.request_repaint();
         });
         if rejected {
            return Ok(JsonRpcResponse::error(
               USER_REJECTED_REQUEST,
               payload.id,
            ));
         }
         error!("Error sending tx: {:?}", e);
         return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
      }
   };

   // Update balances
   RT.spawn(async move {
      let transact_to_exists = ctx.wallet_exists(transact_to);
      let manager = ctx.balance_manager();

      match manager.update_eth_balance(ctx.clone(), chain.id(), vec![from], true).await {
         Ok(_) => {}
         Err(e) => {
            tracing::error!("Error updating ETH balance: {:?}", e);
         }
      }

      if transact_to_exists {
         match manager
            .update_eth_balance(ctx.clone(), chain.id(), vec![transact_to], true)
            .await
         {
            Ok(_) => {}
            Err(e) => {
               tracing::error!("Error updating ETH balance: {:?}", e);
            }
         }
      }

      // Update token balances if needed
      let erc20_transfers = &tx_rich.analysis.erc20_transfers();
      let eth_wraps = &tx_rich.analysis.eth_wraps();
      let eth_unwraps = &tx_rich.analysis.weth_unwraps();

      for wrap in eth_wraps {
         let token = ERC20Token::wrapped_native_token(chain.id());
         let recipient = wrap.recipient;
         let recipient_exists = ctx.wallet_exists(recipient);

         if recipient_exists {
            match manager
               .update_tokens_balance(
                  ctx.clone(),
                  chain.id(),
                  recipient,
                  vec![token],
                  true,
               )
               .await
            {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!("Error updating token balance: {:?}", e);
               }
            }

            ctx.update_public_data(chain.id(), recipient);
         }
      }

      for unwrap in eth_unwraps {
         let token = ERC20Token::wrapped_native_token(chain.id());
         let src = unwrap.src;
         let src_exists = ctx.wallet_exists(src);

         if src_exists {
            match manager
               .update_tokens_balance(ctx.clone(), chain.id(), src, vec![token], true)
               .await
            {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!("Error updating token balance: {:?}", e);
               }
            }

            ctx.update_public_data(chain.id(), src);
         }
      }

      for transfer in erc20_transfers {
         let token = transfer.currency.to_erc20().into_owned();
         let sender = transfer.sender;
         let recipient = transfer.recipient;
         let sender_exists = ctx.wallet_exists(sender);
         let recipient_exists = ctx.wallet_exists(recipient);

         if sender_exists {
            match manager
               .update_tokens_balance(
                  ctx.clone(),
                  chain.id(),
                  sender,
                  vec![token.clone()],
                  true,
               )
               .await
            {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!("Error updating token balance: {:?}", e);
               }
            }

            ctx.update_public_data(chain.id(), sender);
         }

         if recipient_exists {
            match manager
               .update_tokens_balance(
                  ctx.clone(),
                  chain.id(),
                  recipient,
                  vec![token],
                  true,
               )
               .await
            {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!("Error updating token balance: {:?}", e);
               }
            }

            ctx.update_public_data(chain.id(), recipient);
         }

         if transact_to_exists {
            ctx.update_public_data(chain.id(), transact_to);
         }
      }
   });

   let hash = receipt.transaction_hash;
   let hex_hash = hex::encode(hash);
   let hash_str = format!("0x{}", hex_hash);

   let response = JsonRpcResponse::ok(Some(Value::String(hash_str)), payload.id);
   Ok(response)
}

async fn handle_request(
   ctx: ZeusCtx,
   origin: String,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let method = match RequestMethod::from_str(&payload.method) {
      Ok(method) => method,
      Err(e) => {
         error!("Unsupported method: {:?}", e);
         return Ok(JsonRpcResponse::error(
            UNSUPPORTED_METHOD,
            payload.id,
         ));
      }
   };

   #[cfg(feature = "dev")]
   info!(
      "Received request '{}' from dapp: {}",
      method.as_str(),
      origin
   );

   let dapp_connected = ctx.is_dapp_connected(&origin);

   if !dapp_connected {
      return match method {
         RequestMethod::RequestAccounts | RequestMethod::WalletRequestPermissions => {
            info!(
               "Dapp {} not connected, Requested connection with method {}",
               origin,
               method.as_str()
            );
            connect(ctx, origin, payload, method).await
         }
         RequestMethod::EthAccounts => request_accounts(ctx, &origin, payload),
         RequestMethod::WalletGetPermissions => get_permissions(ctx, &origin, payload),
         RequestMethod::ChainId => chain_id(ctx, payload),
         _ => {
            error!(
               "Dapp at origin '{}' is not connected and tried to call method '{}'.",
               origin,
               method.as_str()
            );
            Ok(JsonRpcResponse::error(UNAUTHORIZED, payload.id))
         }
      };
   }

   match method {
      RequestMethod::BlockNumber => block_number(ctx, payload).await,
      RequestMethod::ChainId => chain_id(ctx, payload),
      RequestMethod::EthGasPrice => get_gas_price(ctx, payload),
      RequestMethod::EthMaxPriorityFeePerGas => max_priority_fee_per_gas(ctx, payload).await,
      RequestMethod::GetBalance => get_balance(ctx, payload),
      RequestMethod::EthCall => eth_call(ctx, payload).await,
      RequestMethod::EstimateGas => estimate_gas(ctx, payload).await,
      RequestMethod::EthAccounts | RequestMethod::RequestAccounts => {
         request_accounts(ctx, &origin, payload)
      }
      RequestMethod::WalletGetPermissions | RequestMethod::WalletRequestPermissions => {
         get_permissions(ctx, &origin, payload)
      }
      RequestMethod::WalletGetCapabilities => get_capabilities(payload),
      RequestMethod::EthGetCode => eth_get_code(ctx, payload).await,
      RequestMethod::EthGetStorageAt => eth_get_storage_at(ctx, payload).await,
      RequestMethod::WalletRevokePermissions => wallet_revoke_permissions(ctx, origin, payload),
      RequestMethod::EthSignedTypedDataV4 => eth_sign_typed_data_v4(ctx, origin, payload).await,
      RequestMethod::PersonalSign => personal_sign(ctx, origin, payload).await,
      RequestMethod::EthSendTransaction => eth_send_transaction(ctx, origin, payload).await,
      RequestMethod::WalletSwitchEthereumChain | RequestMethod::WalletAddEthereumChain => {
         switch_ethereum_chain(ctx, origin, payload).await
      }
      RequestMethod::EthGetTransactionReceipt => eth_get_transaction_receipt(ctx, payload).await,
      RequestMethod::EthGetTransactionByHash => eth_get_transaction_by_hash(ctx, payload).await,
      RequestMethod::EthGetBlockByNumber => eth_get_block_by_number(ctx, payload).await,
   }
}

// Handler for POST /api (JSON-RPC)
async fn api_handler(
   origin: String,
   ctx: ZeusCtx,
   body: ApiRequestBody,
) -> Result<impl warp::Reply, Infallible> {
   let payload = body.rpc_request;
   let response_body = handle_request(ctx, origin, payload).await?;

   Ok(warp::reply::json(&response_body))
}

fn with_ctx(ctx: ZeusCtx) -> impl Filter<Extract = (ZeusCtx,), Error = Infallible> + Clone {
   warp::any().map(move || ctx.clone())
}

#[derive(Debug)]
struct Unauthorized;

impl warp::reject::Reject for Unauthorized {}

fn with_pairing_token(expected: String) -> impl Filter<Extract = (), Error = Rejection> + Clone {
   warp::header::optional::<String>(TOKEN_HEADER)
      .and_then(move |provided: Option<String>| {
         let expected = expected.clone();
         async move {
            match provided {
               Some(provided) if token_matches(&expected, &provided) => Ok(()),
               _ => Err(warp::reject::custom(Unauthorized)),
            }
         }
      })
      .untuple_one()
}

fn with_dapp_origin() -> impl Filter<Extract = (String,), Error = Rejection> + Clone {
   warp::header::optional::<String>(ORIGIN_HEADER).and_then(|provided: Option<String>| async move {
      match provided.as_deref().map(parse_dapp_origin) {
         Some(Ok(origin)) => Ok(origin),
         _ => Err(warp::reject::custom(Unauthorized)),
      }
   })
}

async fn handle_rejection(err: Rejection) -> Result<impl warp::Reply, std::convert::Infallible> {
   if err.find::<Unauthorized>().is_some() {
      return Ok(warp::reply::with_status(
         "Unauthorized",
         StatusCode::UNAUTHORIZED,
      ));
   }
   Ok(warp::reply::with_status(
      "Internal Server Error",
      StatusCode::INTERNAL_SERVER_ERROR,
   ))
}

pub async fn run_server(ctx: ZeusCtx) -> Result<(), Box<dyn std::error::Error>> {
   let token = generate_pairing_token();
   let preferred = ctx.server_port();
   let (listener, port) = match bind_connector_listener(preferred).await {
      Ok(bound) => bound,
      Err(e) => {
         error!(
            "Cannot bind connector on 127.0.0.1 starting at {}: {}",
            preferred, e
         );
         return Err(e.into());
      }
   };
   ctx.write(|ctx| ctx.server_port = port);

   let session = ConnectorSession {
      token: token.clone(),
      port,
   };
   let session_path = connector_session_path()?;
   write_connector_session(&session_path, &session)?;
   info!(
      "Wrote connector session to {} (port {})",
      session_path.display(),
      port
   );

   match (std::env::current_exe(), std::env::current_dir()) {
      (Ok(exe), Ok(cwd)) => {
         if let Err(e) = register_native_host(&exe, &cwd) {
            warn!("Failed to register connector native host: {e}");
         }
      }
      (Err(e), _) => warn!("Cannot resolve Zeus binary for native host: {e}"),
      (_, Err(e)) => warn!("Cannot resolve working directory for native host: {e}"),
   }

   // Filter for GET /status
   let status_route = warp::path!("status")
      .and(warp::get())
      .and(with_pairing_token(token.clone()))
      .and(with_ctx(ctx.clone()))
      .and_then(status_handler);

   // Filter for POST /api
   let api_route = warp::path!("api")
      .and(warp::post())
      .and(with_pairing_token(token))
      .and(with_dapp_origin())
      .and(with_ctx(ctx.clone()))
      .and(warp::body::json::<ApiRequestBody>())
      .and_then(api_handler);

   // Combine Routes — no CORS: browser pages must not read this API.
   let routes = status_route
      .or(api_route)
      .with(warp::trace::request())
      .recover(handle_rejection);

   ctx.write(|ctx| ctx.server_running = true);
   info!(
      "Zeus (warp) RPC server listening on 127.0.0.1:{}",
      port
   );

   warp::serve(routes).incoming(listener).run().await;

   ctx.write(|ctx| ctx.server_running = false);
   info!("Zeus (warp) RPC server stopped");

   Ok(())
}

#[cfg(test)]
mod connector_auth_tests {
   use super::*;

   #[test]
   fn json_body_origin_is_ignored() {
      let body: ApiRequestBody = serde_json::from_str(
         r#"{
            "origin": "https://app.uniswap.org",
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_sendTransaction",
            "params": []
         }"#,
      )
      .unwrap();
      assert_eq!(body.rpc_request.method, "eth_sendTransaction");
      assert_eq!(
         body._origin.as_deref(),
         Some("https://app.uniswap.org")
      );
   }

   #[test]
   fn user_reject_errors_are_detected() {
      assert!(is_user_rejected(&anyhow!("Transaction rejected")));
      assert!(is_user_rejected(&anyhow!(
         "You cancelled the signing process"
      )));
      assert!(!is_user_rejected(&anyhow!("RPC timeout")));
   }

   #[test]
   fn rpc_quantities_are_unpadded_hex() {
      assert_eq!(hex_quantity_u64(0), "0x0");
      assert_eq!(hex_quantity_u64(8453), "0x2105");
      assert_eq!(hex_quantity_u256(U256::ZERO), "0x0");
      assert_eq!(hex_data(&[]), "0x");
      assert_eq!(hex_data(&[0xab, 0xcd]), "0xabcd");
   }

   #[test]
   fn parse_hex_chain_id_accepts_0x_prefix() {
      assert_eq!(parse_hex_chain_id("0x2105"), Some(8453));
      assert_eq!(parse_hex_chain_id("0X1"), Some(1));
      assert_eq!(parse_hex_chain_id("8453"), None);
   }

   #[test]
   fn unknown_chain_is_4902() {
      let params = json!([{ "chainId": "0x89" }]); // Polygon
      assert_eq!(
         parse_requested_chain(&params),
         Err(UNRECOGNIZED_CHAIN)
      );
      let base = json!([{ "chainId": "0x2105" }]);
      assert_eq!(parse_requested_chain(&base), Ok(ChainId::Base));
   }

   #[test]
   fn parse_rpc_u256_and_bytes_defaults() {
      assert_eq!(parse_rpc_u256(None), Ok(U256::ZERO));
      assert_eq!(parse_rpc_u256(Some(&json!("0x"))), Ok(U256::ZERO));
      assert_eq!(
         parse_rpc_u256(Some(&json!("0xa"))),
         Ok(U256::from(10u64))
      );
      assert_eq!(parse_rpc_bytes(None), Ok(Bytes::new()));
      assert_eq!(
         parse_rpc_bytes(Some(&json!("0x"))),
         Ok(Bytes::new())
      );
   }

   #[test]
   fn parse_rpc_block_id_defaults_to_latest() {
      assert_eq!(
         parse_rpc_block_id(None, "eth_getCode").unwrap(),
         BlockId::latest()
      );
      assert_eq!(
         parse_rpc_block_id(Some(&json!("latest")), "eth_getCode").unwrap(),
         BlockId::latest()
      );
   }

   #[test]
   fn parse_rpc_tx_call_allows_missing_data() {
      let object = match json!({ "to": "0x0000000000000000000000000000000000000001" }) {
         Value::Object(map) => map,
         _ => unreachable!(),
      };
      let call = parse_rpc_tx_call(&object, Address::ZERO, "eth_call").unwrap();
      assert_eq!(
         call.to,
         Address::from_str("0x0000000000000000000000000000000000000001").unwrap()
      );
      assert!(call.data.is_empty());
      assert_eq!(call.value, U256::ZERO);
      assert_eq!(call.from, Address::ZERO);
   }

   #[test]
   fn wallet_get_capabilities_returns_empty_object() {
      let payload = JsonRpcRequest {
         jsonrpc: "2.0".to_string(),
         id: json!(1),
         method: "wallet_getCapabilities".to_string(),
         params: json!([]),
      };
      let res = get_capabilities(payload).unwrap();
      assert!(res.error.is_none());
      assert_eq!(res.result, Some(json!({})));
   }

   #[test]
   fn curve_rpc_methods_are_recognized() {
      assert_eq!(
         RequestMethod::from_str("eth_maxPriorityFeePerGas").unwrap(),
         RequestMethod::EthMaxPriorityFeePerGas
      );
      assert_eq!(
         RequestMethod::from_str("eth_getBlockByNumber").unwrap(),
         RequestMethod::EthGetBlockByNumber
      );
   }

   #[test]
   fn connector_port_candidates_prefer_then_decrement() {
      let ports: Vec<u16> = connector_port_candidates(65534).collect();
      assert_eq!(ports[0], 65534);
      assert_eq!(ports[1], 65533);
      assert_eq!(ports.len(), CONNECTOR_PORT_ATTEMPTS as usize);
      assert!(!ports.contains(&0));
   }

   #[tokio::test]
   async fn bind_connector_falls_back_when_preferred_is_in_use() {
      let occupied = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
         .await
         .unwrap();
      let preferred = occupied.local_addr().unwrap().port();
      let (listener, port) = bind_connector_listener(preferred).await.unwrap();
      assert_ne!(port, preferred);
      assert!(connector_port_candidates(preferred).any(|p| p == port));
      drop(listener);
      drop(occupied);
   }
}
