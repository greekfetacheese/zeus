use crate::core::ZeusCtx;
use crate::core::utils::{RT, eth};
use crate::gui::SHARED_GUI;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::convert::Infallible;
use std::net::SocketAddr;
use tracing::{error, info};
use warp::Filter;

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
         RequestMethod::GetBalance,
         RequestMethod::EthSignedTypedDataV4,
         RequestMethod::PersonalSign,
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
   let accounts = vec![ctx.current_wallet_address().to_string()];
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
   let current_wallet = ctx.current_wallet_address();
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
   let current_wallet = ctx.current_wallet_address().to_string();
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

   let current_wallet = ctx.current_wallet_address().to_string();

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
   let block = ctx.get_latest_block().await;
   let block_number = match block {
      Ok(Some(block)) => block.number,
      Ok(None) => 0,
      Err(e) => {
         error!("Error getting latest block: {:?}", e);
         return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
      }
   };

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

   let chain = ctx.chain().id();
   let client = match ctx.get_client(chain).await {
      Ok(client) => client,
      Err(e) => {
         error!("Error getting client: {:?}", e);
         return Ok(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: payload.id,
            result: None,
            error: Some(JsonRpcError::internal_error()),
         });
      }
   };

   let storage = match client.get_storage_at(address, slot).block_id(block).await {
      Ok(storage) => storage,
      Err(e) => {
         error!("Error getting storage: {:?}", e);
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

   let chain = ctx.chain().id();
   let client = match ctx.get_client(chain).await {
      Ok(client) => client,
      Err(e) => {
         error!("Error getting client: {:?}", e);
         return Ok(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: payload.id,
            result: None,
            error: Some(JsonRpcError::internal_error()),
         });
      }
   };

   let code = match client.get_code_at(address).block_id(block).await {
      Ok(code) => code,
      Err(e) => {
         error!("Error getting code: {:?}", e);
         return Ok(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: payload.id,
            result: None,
            error: Some(JsonRpcError::internal_error()),
         });
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

   let chain = ctx.chain().id();
   let client = match ctx.get_client(chain).await {
      Ok(client) => client,
      Err(e) => {
         error!("Error getting client: {:?}", e);
         return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
      }
   };

   let tx = match client.get_transaction_by_hash(hash).await {
      Ok(tx_opt) => tx_opt,
      Err(e) => {
         error!("Error fetching transaction by hash: {:?}", e);
         return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
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

   let chain = ctx.chain().id();
   let client = match ctx.get_client(chain).await {
      Ok(client) => client,
      Err(e) => {
         error!("Error getting client: {:?}", e);
         return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
      }
   };

   let receipt = match client.get_transaction_receipt(hash).await {
      Ok(receipt_opt) => receipt_opt,
      Err(e) => {
         error!("Error fetching transaction receipt: {:?}", e);
         return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
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
   let client = match ctx.get_client(chain).await {
      Ok(client) => client,
      Err(e) => {
         error!("Error getting client: {:?}", e);
         return Ok(JsonRpcResponse::error(INTERNAL_ERROR, payload.id));
      }
   };

   let from = ctx.current_wallet_address();

   let tx = TransactionRequest::default().with_from(from).with_to(to).with_input(calldata);

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
   let current_wallet = ctx.current_wallet_address();
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

   // Prefix for personal_sign: "\x19Ethereum Signed Message:\n<length>" + message
   let prefix = format!(
      "\x19Ethereum Signed Message:\n{}",
      message_bytes.len()
   );
   let full_message = format!(
      "{}{}",
      prefix,
      String::from_utf8_lossy(&message_bytes)
   );

   let msg = json!(full_message);
   let chain = ctx.chain();
   let signature = match eth::sign_message(ctx, origin, chain, msg).await {
      Ok(sig) => sig,
      Err(e) => {
         SHARED_GUI.write(|gui| {
            gui.loading_window.reset();
            gui.msg_window.open("Error Signing Message", e.to_string());
            gui.request_repaint();
         });
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
      gui.header.set_current_chain(chain);
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

   let transact_to = match Address::from_str(to_str) {
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
   let auth_list = Vec::new();

   let (receipt, tx_rich) = match eth::send_transaction(
      ctx.clone(),
      origin.clone(),
      None,
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
         SHARED_GUI.write(|gui| {
            gui.loading_window.reset();
            gui.notification.reset();
            gui.tx_confirmation_window.reset(ctx.clone());
            gui.msg_window.open("Error Sending Transaction", e.to_string());
            gui.request_repaint();
         });
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
         match manager.update_eth_balance(ctx.clone(), chain.id(), vec![transact_to], true).await {
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
               .update_tokens_balance(ctx.clone(), chain.id(), recipient, vec![token], true)
               .await
            {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!("Error updating token balance: {:?}", e);
               }
            }

            ctx.calculate_portfolio_value(chain.id(), recipient);
         }
      }

      for unwrap in eth_unwraps {
         let token = ERC20Token::wrapped_native_token(chain.id());
         let src = unwrap.src;
         let src_exists = ctx.wallet_exists(src);

         if src_exists {
            match manager.update_tokens_balance(ctx.clone(), chain.id(), src, vec![token], true).await {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!("Error updating token balance: {:?}", e);
               }
            }

            ctx.calculate_portfolio_value(chain.id(), src);
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
                  true
               )
               .await
            {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!("Error updating token balance: {:?}", e);
               }
            }

            ctx.calculate_portfolio_value(chain.id(), sender);
         }

         if recipient_exists {
            match manager
               .update_tokens_balance(ctx.clone(), chain.id(), recipient, vec![token], true)
               .await
            {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!("Error updating token balance: {:?}", e);
               }
            }

            ctx.calculate_portfolio_value(chain.id(), recipient);
         }

         if transact_to_exists {
            ctx.calculate_portfolio_value(chain.id(), transact_to);
         }

         ctx.save_balance_manager();
         ctx.save_portfolio_db();
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
      m if m == RequestMethod::GetBalance => get_balance(ctx, payload),
      m if m == RequestMethod::EthCall => eth_call(ctx, payload).await,
      m if m == RequestMethod::EstimateGas => estimate_gas(ctx, payload).await,
      m if m == RequestMethod::WalletGetPermissions => get_permissions(ctx, &origin, payload).await,
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

      m if m == RequestMethod::WalletSwitchEthereumChain => switch_ethereum_chain(ctx, payload),

      m if m == RequestMethod::EthGetTransactionReceipt => {
         eth_get_transaction_receipt(ctx, payload).await
      }

      m if m == RequestMethod::EthGetTransactionByHash => {
         eth_get_transaction_by_hash(ctx, payload).await
      }

      _ => Ok(JsonRpcResponse::error(
         UNSUPPORTED_METHOD,
         payload.id,
      )),
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
   let routes = status_route.or(api_route).with(cors).with(warp::trace::request());

   let port = ctx.server_port();
   let addr = SocketAddr::from(([127, 0, 0, 1], port));
   info!("Zeus (warp) RPC server listening on {}", addr);

   warp::serve(routes).run(addr).await;

   Ok(())
}
