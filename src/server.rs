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

/// Default server port
pub const SERVER_PORT: u16 = 65534;

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

   pub fn is_connection_method(&self) -> bool {
      matches!(
         self,
         RequestMethod::RequestAccounts | RequestMethod::WalletRequestPermissions
      )
   }

   pub fn supported_methods() -> Vec<RequestMethod> {
      vec![
         RequestMethod::WalletAddEthereumChain,
         RequestMethod::WalletSwitchEthereumChain,
         RequestMethod::WalletGetPermissions,
         RequestMethod::WalletGetCapabilities,
         RequestMethod::WalletRequestPermissions,
         RequestMethod::WalletRevokePermissions,
         RequestMethod::EthGetTransactionByHash,
         RequestMethod::EthGetTransactionReceipt,
         RequestMethod::EthGetBlockByNumber,
         RequestMethod::EthAccounts,
         RequestMethod::RequestAccounts,
         RequestMethod::EthSendTransaction,
         RequestMethod::BlockNumber,
         RequestMethod::EthCall,
         RequestMethod::EthGetCode,
         RequestMethod::EthGetStorageAt,
         RequestMethod::ChainId,
         RequestMethod::EstimateGas,
         RequestMethod::EthGasPrice,
         RequestMethod::EthMaxPriorityFeePerGas,
         RequestMethod::GetBalance,
         RequestMethod::EthSignedTypedDataV4,
         RequestMethod::PersonalSign,
      ]
   }
}

const SAFE_UNCONNECTED_METHODS: &[RequestMethod] = &[RequestMethod::ChainId];

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StatusResponse {
   pub status: bool,
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
         code: code,
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

async fn request_accounts(
   ctx: ZeusCtx,
   origin: &str,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let current_wallet = ctx.current_wallet_info().address;
   let connected = ctx.is_dapp_connected(origin);

   if connected {
      Ok(JsonRpcResponse {
         jsonrpc: "2.0".to_string(),
         id: payload.id,
         result: Some(json!(vec![current_wallet.to_string()])),
         error: None,
      })
   } else {
      return Ok(JsonRpcResponse {
         jsonrpc: "2.0".to_string(),
         id: payload.id,
         result: Some(json!([])),
         error: None,
      });
   }
}

async fn get_permissions(
   ctx: ZeusCtx,
   origin: &str,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let current_wallet = ctx.current_wallet_info().address.to_string();
   let connected = ctx.is_dapp_connected(origin);

   if connected {
      Ok(JsonRpcResponse {
         jsonrpc: "2.0".to_string(),
         id: payload.id,
         result: Some(json!([{
             "parentCapability": "eth_accounts",
             "caveats": [{
                 "type": "restrictReturnedAccounts",
                 "value": [current_wallet]
             }]
         }])),
         error: None,
      })
   } else {
      return Ok(JsonRpcResponse {
         jsonrpc: "2.0".to_string(),
         id: payload.id,
         result: Some(json!([])),
         error: None,
      });
   }
}

async fn get_capabilities(
   _ctx: ZeusCtx,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id))
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
   method: RequestMethod, // New param
) -> Result<JsonRpcResponse, Infallible> {
   SHARED_GUI.write(|gui| {
      gui.confirm_window.open("Connect to Dapp");
      gui.confirm_window.set_msg2(origin.clone());
   });

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

   Ok(JsonRpcResponse {
      jsonrpc: "2.0".to_string(),
      id: payload.id,
      result,
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
      result: Some(json!(hex_quantity_u256(balance.wei()))),
      error: None,
   };

   Ok(response)
}

async fn eth_get_storage_at(
   ctx: ZeusCtx,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let array = match payload.params {
      Value::Array(arr) => arr,
      _ => {
         error!("Invalid params for eth_getStorageAt: params is not an array");
         return Ok(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: payload.id,
            result: None,
            error: Some(JsonRpcError::invalid_params()),
         });
      }
   };

   let (address_str, slot_str, block_str) = if array.len() == 3 {
      let address_str = match &array[0] {
         Value::String(s) => s,
         _ => {
            error!("Invalid params for eth_getStorageAt: params[0] is not a string");
            return Ok(JsonRpcResponse {
               jsonrpc: "2.0".to_string(),
               id: payload.id,
               result: None,
               error: Some(JsonRpcError::invalid_params()),
            });
         }
      };

      let slot_str = match &array[1] {
         Value::String(s) => s,
         _ => {
            error!("Invalid params for eth_getStorageAt: params[1] is not a string");
            return Ok(JsonRpcResponse {
               jsonrpc: "2.0".to_string(),
               id: payload.id,
               result: None,
               error: Some(JsonRpcError::invalid_params()),
            });
         }
      };

      let block_str = match &array[2] {
         Value::String(s) => s,
         _ => {
            error!("Invalid params for eth_getStorageAt: params[2] is not a string");
            return Ok(JsonRpcResponse {
               jsonrpc: "2.0".to_string(),
               id: payload.id,
               result: None,
               error: Some(JsonRpcError::invalid_params()),
            });
         }
      };

      (address_str, slot_str, block_str)
   } else {
      error!("Invalid params for eth_getStorageAt: expected array with 3 elements");
      return Ok(JsonRpcResponse {
         jsonrpc: "2.0".to_string(),
         id: payload.id,
         result: None,
         error: Some(JsonRpcError::invalid_params()),
      });
   };

   let address = match Address::from_str(address_str) {
      Ok(address) => address,
      Err(_) => {
         error!("Invalid params for eth_getStorageAt: String is not a valid ethereum address");
         return Ok(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: payload.id,
            result: None,
            error: Some(JsonRpcError::invalid_params()),
         });
      }
   };

   let slot = match U256::from_str(slot_str) {
      Ok(slot) => slot,
      Err(_) => {
         error!("Invalid params for eth_getStorageAt: String is not a valid U256 value");
         return Ok(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: payload.id,
            result: None,
            error: Some(JsonRpcError::invalid_params()),
         });
      }
   };

   let block = match BlockId::from_str(block_str) {
      Ok(block) => block,
      Err(_) => {
         error!("Invalid params for eth_getStorageAt: String is not a valid block id");
         return Ok(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: payload.id,
            result: None,
            error: Some(JsonRpcError::invalid_params()),
         });
      }
   };

   let storage = match ctx.get_storage(block, address, slot).await {
      Ok(storage) => storage,
      Err(_) => {
         return Ok(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: payload.id,
            result: None,
            error: Some(JsonRpcError::internal_error()),
         });
      }
   };

   let bytes = storage.to_be_bytes_vec();
   let hex = hex::encode(bytes);
   let res = format!("0x{}", hex);

   let response = JsonRpcResponse {
      jsonrpc: "2.0".to_string(),
      id: payload.id,
      result: Some(Value::String(res)),
      error: None,
   };

   Ok(response)
}

async fn eth_get_code(
   ctx: ZeusCtx,
   payload: JsonRpcRequest,
) -> Result<JsonRpcResponse, Infallible> {
   let array = match payload.params {
      Value::Array(arr) => arr,
      _ => {
         error!("Invalid params for eth_getCode: params is not an array");
         return Ok(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: payload.id,
            result: None,
            error: Some(JsonRpcError::invalid_params()),
         });
      }
   };

   let (address_str, block_str) = if array.len() == 2 {
      let address_str = match &array[0] {
         Value::String(s) => s,
         _ => {
            error!("Invalid params for eth_getCode: params[0] is not a string");
            return Ok(JsonRpcResponse {
               jsonrpc: "2.0".to_string(),
               id: payload.id,
               result: None,
               error: Some(JsonRpcError::invalid_params()),
            });
         }
      };

      let block_str = match &array[1] {
         Value::String(s) => s,
         _ => {
            error!("Invalid params for eth_getCode: params[1] is not a string");
            return Ok(JsonRpcResponse {
               jsonrpc: "2.0".to_string(),
               id: payload.id,
               result: None,
               error: Some(JsonRpcError::invalid_params()),
            });
         }
      };

      (address_str, block_str)
   } else {
      error!("Invalid params for eth_getCode: expected array with 2 elements");
      return Ok(JsonRpcResponse {
         jsonrpc: "2.0".to_string(),
         id: payload.id,
         result: None,
         error: Some(JsonRpcError::invalid_params()),
      });
   };

   let address = match Address::from_str(address_str) {
      Ok(address) => address,
      Err(_) => {
         error!("Invalid params for eth_getCode: String is not a valid ethereum address");
         return Ok(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: payload.id,
            result: None,
            error: Some(JsonRpcError::invalid_params()),
         });
      }
   };

   let block = match BlockId::from_str(block_str) {
      Ok(block) => block,
      Err(_) => {
         error!("Invalid params for eth_getCode: String is not a valid block id");
         return Ok(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: payload.id,
            result: None,
            error: Some(JsonRpcError::invalid_params()),
         });
      }
   };

   let code = match ctx.get_code(block, address).await {
      Ok(code) => code,
      Err(e) => {
         let err = JsonRpcError::new(INTERNAL_ERROR, e.to_string(), None);
         return Ok(JsonRpcResponse::error_res(err, payload.id));
      }
   };

   let result = hex::encode(code);

   let response = JsonRpcResponse {
      jsonrpc: "2.0".to_string(),
      id: payload.id,
      result: Some(Value::String(format!("0x{}", result))),
      error: None,
   };

   Ok(response)
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

   let (calldata_str, to_str) = match (
      params_object.get("data").or_else(|| params_object.get("input")),
      params_object.get("to"),
   ) {
      (Some(Value::String(calldata)), Some(Value::String(to))) => (calldata, to),
      _ => {
         return {
            error!(
               "Invalid params for eth_call, data/input and to are not strings {:#?}",
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

   let from = ctx.current_wallet_info().address;

   let tx = TransactionRequest::default().with_from(from).with_to(to).with_input(calldata);

   let output = match ctx.get_eth_call(tx).await {
      Ok(output) => output,
      Err(e) => {
         let err = JsonRpcError::new(INTERNAL_ERROR, e.to_string(), None);
         return Ok(JsonRpcResponse::error_res(err, payload.id));
      }
   };

   let result = hex_data(&output.result);

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
      result: Some(json!(hex_quantity_u64(gas_price.next))),
      error: None,
   };

   Ok(response)
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

   let to_str = match rpc_opt_string(params_object, "to") {
      Some(s) => s,
      None => {
         error!(
            "Invalid params for eth_estimateGas, missing 'to' {:#?}",
            params_object
         );
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
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

   let from = match rpc_opt_string(params_object, "from") {
      Some(from_str) => match Address::from_str(from_str) {
         Ok(from) => from,
         Err(_) => {
            error!(
               "Invalid params for eth_estimateGas, String is not a valid ethereum address {:#?}",
               from_str
            );
            return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
         }
      },
      None => ctx.current_wallet_info().address,
   };

   let data_val = params_object.get("data").or_else(|| params_object.get("input"));
   let calldata = match parse_rpc_bytes(data_val) {
      Ok(calldata) => calldata,
      Err(_) => {
         error!(
            "Invalid params for eth_estimateGas, data/input is not valid bytes {:#?}",
            data_val
         );
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
      }
   };

   let value = match parse_rpc_u256(params_object.get("value")) {
      Ok(value) => value,
      Err(_) => {
         error!(
            "Invalid params for eth_estimateGas, value is not a valid U256 {:#?}",
            params_object.get("value")
         );
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
      }
   };

   let chain = ctx.chain().id();
   let client = match ctx.get_client(chain).await {
      Ok(client) => client,
      Err(e) => {
         error!("Error getting client: {:?}", e);
         return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
      }
   };

   let tx = TransactionRequest::default()
      .with_from(from)
      .with_to(to)
      .with_input(calldata)
      .with_value(value);

   let gas = match client.estimate_gas(tx).await {
      Ok(output) => output,
      Err(e) => {
         let err = JsonRpcError::new(INTERNAL_ERROR, e.to_string(), None);
         return Ok(JsonRpcResponse::error_res(err, payload.id));
      }
   };

   let response = JsonRpcResponse {
      jsonrpc: "2.0".to_string(),
      id: payload.id,
      result: Some(json!(hex_quantity_u64(gas))),
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

   let sig_bytes = signature.as_bytes();
   let sig_hex = hex::encode(sig_bytes);
   let sig_hex = format!("0x{}", sig_hex);

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

   let sig_bytes = signature.as_bytes();
   let sig_hex = format!("0x{}", hex::encode(sig_bytes));

   Ok(JsonRpcResponse::ok(
      Some(Value::String(sig_hex)),
      payload.id,
   ))
}

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

/// Zeus only knows a fixed chain set. If the requested chain is supported,
/// confirm and switch; otherwise 4902 (same as an unknown switch).
async fn add_ethereum_chain(
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
   let params_array = match params {
      Value::Array(params) => params,
      _ => {
         error!(
            "Invalid params for chain switch/add, params is not an array {:#?}",
            params
         );
         return Err(INVALID_PARAMS);
      }
   };

   let object = match params_array.first() {
      Some(Value::Object(params)) => params,
      _ => {
         error!(
            "Invalid params for chain switch/add, params[0] is not an object {:#?}",
            params_array
         );
         return Err(INVALID_PARAMS);
      }
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
      return Ok(JsonRpcResponse {
         jsonrpc: "2.0".to_string(),
         id: payload_id,
         result: Some(Value::Null),
         error: None,
      });
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

   let mut confirmed = None;
   loop {
      tokio::time::sleep(std::time::Duration::from_millis(100)).await;
      SHARED_GUI.read(|gui| {
         confirmed = gui.confirm_window.get_confirm();
      });
      if confirmed.is_some() {
         SHARED_GUI.write(|gui| {
            gui.confirm_window.reset();
         });
         break;
      }
   }

   if !confirmed.unwrap() {
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

   Ok(JsonRpcResponse {
      jsonrpc: "2.0".to_string(),
      id: payload_id,
      result: Some(Value::Null),
      error: None,
   })
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

   let data_val = object.get("data").or_else(|| object.get("input"));
   let call_data = match parse_rpc_bytes(data_val) {
      Ok(data) => data,
      Err(_) => {
         error!(
            "Invalid params for eth_sendTransaction, data/input is not valid bytes {:#?}",
            data_val
         );
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
      }
   };

   let from = match rpc_opt_string(object, "from") {
      Some(from_str) => match Address::from_str(from_str) {
         Ok(from) => from,
         Err(_) => {
            error!(
               "Invalid params for eth_sendTransaction, String is not a valid ethereum address {:#?}",
               from_str
            );
            return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
         }
      },
      None => ctx.current_wallet_info().address,
   };

   let to_str = match rpc_opt_string(object, "to") {
      Some(s) => s,
      None => {
         error!(
            "Invalid params for eth_sendTransaction, to is not a string {:#?}",
            object
         );
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
      }
   };

   let value = match parse_rpc_u256(object.get("value")) {
      Ok(v) => v,
      Err(_) => {
         error!(
            "Invalid params for eth_sendTransaction, value is not valid {:#?}",
            object.get("value")
         );
         return Ok(JsonRpcResponse::error(INVALID_PARAMS, payload.id));
      }
   };

   let transact_to = match Address::from_str(&to_str) {
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

   let chain = ctx.chain();
   let auth_list = Vec::new();

   let (receipt, tx_rich) = match send_transaction(
      ctx.clone(),
      origin.clone(),
      None,
      chain,
      true,
      from,
      transact_to,
      call_data,
      value,
      auth_list,
   )
   .await
   {
      Ok(res) => res,
      Err(e) => {
         let rejected = is_user_rejected(&e);
         SHARED_GUI.write(|gui| {
            gui.loading_window.reset();
            gui.notification.reset();
            ctx.write(|ctx| {
               gui.tx_confirmation_window.reset(ctx);
            });
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

// TODO: Apply rate limit
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

   let is_connection_method = method.is_connection_method();
   let dapp_connected = ctx.is_dapp_connected(&origin);

   if method == RequestMethod::EthAccounts {
      return request_accounts(ctx, &origin, payload).await;
   }

   if method == RequestMethod::WalletGetPermissions {
      return get_permissions(ctx, &origin, payload).await;
   }

   if !dapp_connected {
      if is_connection_method {
         info!(
            "Dapp {} not connected, Requested connection with method {}",
            origin,
            method.as_str()
         );
         return connect(ctx, origin, payload, method).await;
      } else if SAFE_UNCONNECTED_METHODS.contains(&method) {
         // do nothing for now
      } else {
         error!(
            "Dapp at origin '{}' is not connected and tried to call method '{}'.",
            origin,
            method.as_str()
         );
         return Ok(JsonRpcResponse::error(UNAUTHORIZED, payload.id));
      }
   }

   // Dapp is CONNECTED - Handle other methods
   match method {
      m if m == RequestMethod::BlockNumber => block_number(ctx, payload).await,
      m if m == RequestMethod::ChainId => chain_id(ctx, payload),
      m if m == RequestMethod::EthGasPrice => get_gas_price(ctx, payload),
      m if m == RequestMethod::EthMaxPriorityFeePerGas => {
         max_priority_fee_per_gas(ctx, payload).await
      }
      m if m == RequestMethod::GetBalance => get_balance(ctx, payload),
      m if m == RequestMethod::EthCall => eth_call(ctx, payload).await,
      m if m == RequestMethod::EstimateGas => estimate_gas(ctx, payload).await,
      m if m == RequestMethod::WalletGetPermissions => get_permissions(ctx, &origin, payload).await,
      m if m == RequestMethod::RequestAccounts => request_accounts(ctx, &origin, payload).await,
      m if m == RequestMethod::WalletRequestPermissions => {
         get_permissions(ctx, &origin, payload).await
      }
      m if m == RequestMethod::WalletGetCapabilities => get_capabilities(ctx, payload).await,
      m if m == RequestMethod::EthGetCode => eth_get_code(ctx, payload).await,
      m if m == RequestMethod::EthGetStorageAt => eth_get_storage_at(ctx, payload).await,

      m if m == RequestMethod::WalletRevokePermissions => {
         wallet_revoke_permissions(ctx, origin, payload)
      }

      m if m == RequestMethod::EthSignedTypedDataV4 => {
         eth_sign_typed_data_v4(ctx, origin, payload).await
      }

      m if m == RequestMethod::PersonalSign => personal_sign(ctx, origin, payload).await,

      m if m == RequestMethod::EthSendTransaction => {
         eth_send_transaction(ctx, origin, payload).await
      }

      m if m == RequestMethod::WalletSwitchEthereumChain => {
         switch_ethereum_chain(ctx, origin, payload).await
      }

      m if m == RequestMethod::WalletAddEthereumChain => {
         add_ethereum_chain(ctx, origin, payload).await
      }

      m if m == RequestMethod::EthGetTransactionReceipt => {
         eth_get_transaction_receipt(ctx, payload).await
      }

      m if m == RequestMethod::EthGetTransactionByHash => {
         eth_get_transaction_by_hash(ctx, payload).await
      }

      m if m == RequestMethod::EthGetBlockByNumber => eth_get_block_by_number(ctx, payload).await,

      _ => Ok(JsonRpcResponse::error(
         UNSUPPORTED_METHOD,
         payload.id,
      )),
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
   let port = ctx.server_port();
   let session = ConnectorSession {
      token: token.clone(),
      port,
   };
   let session_path = connector_session_path()?;
   write_connector_session(&session_path, &session)?;
   info!(
      "Wrote connector session to {}",
      session_path.display()
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

   let port = ctx.server_port();
   let addr = SocketAddr::from(([127, 0, 0, 1], port));

   let listener = match tokio::net::TcpListener::bind(addr).await {
      Ok(l) => l,
      Err(e) => {
         error!("Cannot bind to {}: {}", addr, e);
         return Err(e.into());
      }
   };

   ctx.write(|ctx| ctx.server_running = true);
   info!("Zeus (warp) RPC server listening on {}", addr);

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
}
