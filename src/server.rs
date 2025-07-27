use crate::core::ZeusCtx;
use crate::core::utils::eth;
use crate::gui::{SHARED_GUI, ui::Step};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::convert::Infallible;
use std::net::SocketAddr;
use tracing::{error, info};
use warp::Filter;

use std::str::FromStr;
use zeus_eth::{
   alloy_network::TransactionBuilder,
   alloy_primitives::{Address, Bytes, U256, hex},
   alloy_provider::Provider,
   alloy_rpc_types::TransactionRequest,
   types::ChainId,
};

/// Default server port
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
   BlockNumber,
   EthCall,
   ChainId,
   EstimateGas,
   EthGasPrice,
   GetBalance,
   EthSignedTypedDataV4,
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
         RequestMethod::BlockNumber => "eth_blockNumber",
         RequestMethod::EthCall => "eth_call",
         RequestMethod::ChainId => "eth_chainId",
         RequestMethod::EstimateGas => "eth_estimateGas",
         RequestMethod::EthGasPrice => "eth_gasPrice",
         RequestMethod::GetBalance => "eth_getBalance",
         RequestMethod::EthSignedTypedDataV4 => "eth_signTypedData_v4",
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
         RequestMethod::BlockNumber,
         RequestMethod::EthCall,
         RequestMethod::ChainId,
         RequestMethod::EstimateGas,
         RequestMethod::EthGasPrice,
         RequestMethod::GetBalance,
         RequestMethod::EthSignedTypedDataV4,
      ]
   }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StatusResponse {
   pub status: bool,
}

#[derive(Deserialize, Debug)]
struct ApiRequestBody {
   origin: String,
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

// Handler for GET /status
async fn status_handler(ctx: ZeusCtx) -> Result<impl warp::Reply, Infallible> {
   let chain = ctx.chain().id_as_hex();
   let current_wallet = ctx.current_wallet();

   let res = json!({
    "status": true,
    "accounts": vec![current_wallet.address_string()],
    "chainId": chain,
   });

   Ok(warp::reply::json(&res))
}

async fn request_accounts(
   ctx: ZeusCtx,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let wallets = ctx.wallets_info();
   let accounts = wallets
      .iter()
      .map(|w| w.address_string())
      .collect::<Vec<_>>();

   Ok(JsonRpcResponse {
      jsonrpc: "2.0".to_string(),
      id: payload.id,
      result: Some(json!(accounts)),
      error: None,
   })
}

/// Aka disconnect
fn wallet_revoke_permissions(
   ctx: ZeusCtx,
   origin: String,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   ctx.disconnect_dapp(&origin);
   Ok(JsonRpcResponse {
      jsonrpc: "2.0".to_string(),
      id: payload.id,
      result: Some(Value::Null),
      error: None,
   })
}

/// Depending on the dapp, we may receive eth_requestAccounts or wallet_getPermissions
/// as the request method for connection
async fn connect(
   ctx: ZeusCtx,
   origin: String,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   // open the confirmation window
   SHARED_GUI.write(|gui| {
      gui.confirm_window.open("Connect to Dapp");
      gui.confirm_window.set_msg2(origin.clone());
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
      let res = JsonRpcResponse::error(USER_REJECTED_REQUEST, payload.id);
      return Ok(res);
   }

   ctx.connect_dapp(origin.clone());
   let current_wallet = ctx.current_wallet().address_string();
   Ok(JsonRpcResponse {
      jsonrpc: "2.0".to_string(),
      id: payload.id,
      result: Some(json!(vec![current_wallet])),
      error: None,
   })
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
   let client = ctx.get_client(chain).await.unwrap();
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

   let address_str = match params_array.first() {
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
   let balance = ctx.get_eth_balance(chain, address);

   let response = JsonRpcResponse {
      jsonrpc: "2.0".to_string(),
      id: payload.id,
      result: Some(json!(balance.wei())),
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

   let params_object = match params_array.first() {
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
   let client = ctx.get_client(chain).await.unwrap();
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
   info!(
      "Received estimateGas params {:#?}",
      payload.params
   );
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

   let params_object = match params_array.first() {
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
   let client = ctx.get_client(chain).await.unwrap();
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
   let signature = match eth::sign_message(ctx, origin.clone(), chain, typed_data_value).await {
      Ok(signature) => signature,
      Err(e) => {
         SHARED_GUI.write(|gui| {
            gui.loading_window.reset();
            gui.msg_window.open("Error Signing Message", e.to_string());
            gui.request_repaint();
         });
         error!("Error signing message: {:?}", e);
         return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
      }
   };

   let sig_bytes = signature.as_bytes();
   let sig_hex = hex::encode(sig_bytes);
   let sig_hex = format!("0x{}", sig_hex);

   let response = JsonRpcResponse::ok(Some(Value::String(sig_hex)), payload.id);
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

   let object = match params_array.first() {
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

   SHARED_GUI.write(|gui| {
      gui.chain_selection.chain_select.chain = chain;
      gui.request_repaint();
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
   origin: String,
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

   let object = match params_array.first() {
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

   let chain = ctx.chain();

   info!("Sending Tx in Chain: {:?}", chain.name());
   info!("From: {:?}", from);
   info!("To: {:?}", to);
   info!("Value: {:?}", value);
   info!("Data: {:?}", call_data);

   let (receipt, tx_rich) = match eth::send_transaction(
      ctx.clone(),
      origin.clone(),
      None,
      None,
      chain,
      true,
      from,
      to,
      call_data,
      value,
   )
   .await
   {
      Ok(res) => res,
      Err(e) => {
         SHARED_GUI.write(|gui| {
            gui.loading_window.reset();
            gui.tx_confirmation_window.reset();
            gui.msg_window
               .open("Error Sending Transaction", e.to_string());
            gui.request_repaint();
         });
         error!("Error sending tx: {:?}", e);
         return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
      }
   };

   let step1 = Step {
      id: "step1",
      in_progress: false,
      finished: true,
      msg: "Transaction Sent".to_string(),
   };

   SHARED_GUI.write(|gui| {
      gui.progress_window
         .open_with(vec![step1], "Success!".to_string());
      gui.progress_window.set_tx(tx_rich);
      gui.request_repaint();
   });

   let hash = receipt.transaction_hash;
   let hash_str = format!("0x{}", hash);

   let response = JsonRpcResponse::ok(Some(Value::String(hash_str)), payload.id);
   Ok(response)
}

async fn handle_request(
   ctx: ZeusCtx,
   origin: String,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let method = payload.method.as_str();
   info!(
      "Received request '{}' from dapp: {}",
      method, origin
   );

   // Methods that can be called WITHOUT a prior connection.
   let is_connection_method = method == RequestMethod::RequestAccounts.as_str()
      || method == RequestMethod::WalletRequestPermissions.as_str()
      || method == RequestMethod::EthAccounts.as_str();

   let dapp_connected = ctx.is_dapp_connected(&origin);

   // If the dapp is not connected, it MUST use a connection method first.
   if !dapp_connected {
      if is_connection_method {
         info!(
            "Dapp not connected. Initiating connection flow for origin: {}",
            origin
         );
         return connect(ctx, origin, payload).await;
      } else {
         error!(
            "Dapp at origin '{}' is not connected and tried to call method '{}'.",
            origin, method
         );
         return Ok(JsonRpcResponse::error(UNAUTHORIZED, payload.id));
      }
   }

   // Dapp is CONNECTED
   match method {
      // Account & Permission Methods
      m if m == RequestMethod::RequestAccounts.as_str() => request_accounts(ctx, payload).await,
      m if m == RequestMethod::EthAccounts.as_str() => request_accounts(ctx, payload).await,

      // Blockchain State Methods
      m if m == RequestMethod::ChainId.as_str() => chain_id(ctx, payload),
      m if m == RequestMethod::BlockNumber.as_str() => block_number(ctx, payload).await,
      m if m == RequestMethod::GetBalance.as_str() => get_balance(ctx, payload),
      m if m == RequestMethod::EthCall.as_str() => eth_call(ctx, payload).await,
      m if m == RequestMethod::EthGasPrice.as_str() => get_gas_price(ctx, payload),
      m if m == RequestMethod::EstimateGas.as_str() => estimate_gas(ctx, payload).await,

      // Transaction & Signing Methods
      m if m == RequestMethod::SendTransaction.as_str() => {
         eth_send_transaction(ctx, origin, payload).await
      }
      m if m == RequestMethod::EthSignedTypedDataV4.as_str() => {
         eth_sign_typed_data_v4(ctx, origin, payload).await
      }

      // Wallet Management Methods
      m if m == RequestMethod::WallletSwitchEthereumChain.as_str() => {
         switch_ethereum_chain(ctx, payload)
      }
      m if m == RequestMethod::WalletRevokePermissions.as_str() => {
         wallet_revoke_permissions(ctx, origin, payload)
      }

      // Unsupported Method
      _ => {
         error!(
            "Method '{}' not supported for a connected dapp.",
            method
         );
         Ok(JsonRpcResponse::error(
            UNSUPPORTED_METHOD,
            payload.id,
         ))
      }
   }
}

// Handler for POST /api (JSON-RPC)
async fn api_handler(ctx: ZeusCtx, body: ApiRequestBody) -> Result<impl warp::Reply, Infallible> {
   let origin = body.origin;
   let payload = body.rpc_request;
   let response_body = handle_request(ctx, origin, payload).await?;

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
      .and(warp::body::json::<ApiRequestBody>())
      .and_then(api_handler);

   // Combine Routes
   let routes = status_route
      .or(api_route)
      .with(cors)
      .with(warp::trace::request());

   let port = ctx.server_port();
   let addr = SocketAddr::from(([127, 0, 0, 1], port));
   info!("Zeus (warp) RPC server listening on {}", addr);

   warp::serve(routes).run(addr).await;

   Ok(())
}
