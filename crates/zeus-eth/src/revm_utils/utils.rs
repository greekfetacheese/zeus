use alloy_rpc_types::Block;
use revm::{
    inspector_handle_register,
    primitives::{Bytes, EVMError, EnvWithHandlerCfg, ResultAndState, SpecId, U256},
    Database, Evm, GetInspector,
};


pub struct InspectRes<DB>
where
    DB: Database,
{
    pub result_state: ResultAndState,
    pub env: EnvWithHandlerCfg,
    pub db: DB,
}

impl<DB> InspectRes<DB>
where
    DB: Database,
{
    pub fn new(result_state: ResultAndState, env: EnvWithHandlerCfg, db: DB) -> Self {
        Self {
            result_state,
            env,
            db,
        }
    }
}



/// Executes the [EnvWithHandlerCfg] against the given [Database] without committing state changes.
pub fn inspect<DB, I>(
    db: DB,
    env: EnvWithHandlerCfg,
    inspector: I,
) -> Result<InspectRes<DB>, EVMError<DB::Error>>
where
    DB: Database,
    I: GetInspector<DB>,
{
    let mut evm = Evm::builder()
        .with_db(db)
        .with_external_context(inspector)
        .with_env_with_handler_cfg(env)
        .append_handler_register(inspector_handle_register)
        .build();
    let res = evm.transact()?;
    let (db, env) = evm.into_db_and_env_with_handler_cfg();
    Ok(InspectRes::new(res, env, db))
}

pub fn new_evm<DB>(db: DB, block: Option<Block>) -> Evm<'static, (), DB>
where
    DB: Database,
{
    let mut evm = Evm::builder()
        .with_db(db)
        .with_spec_id(SpecId::CANCUN)
        .build();

    if let Some(block) = block {
        evm.block_mut().number = U256::from(block.header.number);
        evm.block_mut().timestamp = U256::from(block.header.timestamp);
        evm.block_mut().coinbase = block.header.miner;
    }

    // Disable some checks for easier testing
    evm.cfg_mut().disable_balance_check = true;
    evm.cfg_mut().disable_block_gas_limit = true;
    evm.cfg_mut().disable_base_fee = true;
    evm
}

pub fn revert_msg(bytes: &Bytes) -> String {
    if bytes.len() < 4 {
        return "EVM Returned 0x (Empty Bytes)".to_string();
    }
    let error_data = &bytes[4..];

    match String::from_utf8(error_data.to_vec()) {
        Ok(s) => s.trim_matches(char::from(0)).to_string(),
        Err(_) => "EVM Returned 0x (Empty Bytes)".to_string(),
    }
}