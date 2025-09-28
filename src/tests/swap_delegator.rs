#[cfg(test)]
mod tests {
   use crate::core::{BaseFee, ZeusCtx};
   use crate::gui::ui::dapps::uniswap::swap::get_relevant_pools;
   use crate::utils::{
      swap_quoter::{get_quote, get_quote_with_split_routing},
      zeus_delegate::*,
   };
   use std::str::FromStr;

   use zeus_eth::{
      abi::zeus as zeus_abi,
      alloy_primitives::{Address, Bytes, TxKind, U256},
      alloy_provider::Provider,
      alloy_rpc_types::BlockId,
      alloy_signer::SignerSync,
      amm::uniswap::{AnyUniswapPool, UniswapPool, UniswapV2Pool, UniswapV4Pool},
      currency::{Currency, ERC20Token, NativeCurrency},
      revm_utils::*,
      utils::{NumericValue, SecureSigner, address_book},
   };

   use alloy_eips::eip7702::Authorization;
   use alloy_sol_types::{SolCall, SolValue};
   use either::Either;

   const ZEUS_DELEGATE_CODE: &str = "0x6101006040523461014857604051601f611cbf38819003918201601f19168301916001600160401b03831184841017610134578084926080946040528339810103126101485760405190608082016001600160401b038111838210176101345760405261006b8161014c565b80835261007a6020830161014c565b906020840191825260606100a2816100946040870161014c565b95604088019687520161014c565b94019384526001600160a01b039081166080529051811660a0529051811660c05290511660e052604051611b5e9081610161823960805181818161032101528181610d9201528181610e1e01528181610f2a01526116f1015260a0518181816102de015281816103740152610fff015260c051818181610102015261029b015260e051818181610a2e0152610a7d0152f35b634e487b7160e01b5f52604160045260245ffd5b5f80fd5b51906001600160a01b03821682036101485756fe608080604052600436101561001c575b50361561001a575f80fd5b005b5f3560e01c908162afc4b714610be85750806323a69e7514610a52578063695033f314610a0f57806391dd734614610345578063ad5c464814610302578063e34e7283146102bf578063f73e5aab1461027c5763fa461e331461007f575f61000f565b34610265576001600160a01b03806100f660206100ac61009e36611874565b819593959492940190611947565b604051630b4c774160e11b8152939098166001600160a01b03818116600486015299909216988916602484015262ffffff9097166044830152969094909190859081906064820190565b03816001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000165afa938415610271575f94610231575b506001600160a01b0384168033149081610227575b50156101ca578510156101c25750915b82036101665761001a92611a4f565b60405162461bcd60e51b815260206004820152602e60248201527f556e697377617056335377617043616c6c6261636b3a20616d6f756e74546f5060448201526d30bc90109e9030b6b7bab73a24b760911b6064820152608490fd5b905091610157565b60405162461bcd60e51b815260206004820152602f60248201527f556e697377617056335377617043616c6c6261636b3a204d73672e73656e646560448201526e1c881a5cc81b9bdd0818481c1bdbdb608a1b6064820152608490fd5b905015155f610147565b9093506020813d602011610269575b8161024d6020938361197e565b810103126102655761025e906119a0565b925f610132565b5f80fd5b3d9150610240565b6040513d5f823e3d90fd5b34610265575f3660031901126102655760206040516001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000168152f35b34610265575f3660031901126102655760206040516001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000168152f35b34610265575f3660031901126102655760206040516001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000168152f35b60203660031901126102655760043567ffffffffffffffff811161026557610371903690600401611846565b907f0000000000000000000000000000000000000000000000000000000000000000916001600160a01b038316908133036109a4578201916020818403126102655780359067ffffffffffffffff8211610265570192610120848403126102655760405193610120850185811067ffffffffffffffff821117610975576040526103fa81611923565b855261040860208201611923565b906020860191825260408601906040810135825261042860608201611937565b90606088019182526080810135918260020b8303610265576080890192835260a08201359182151583036102655760a08a0192835261046960c08201611923565b9160c08b0192835260e082013567ffffffffffffffff8111610265578201928a601f85011215610265576001600160a01b036104e0816104cb8f9e8f989760e06104c0610100938b602062ffffff9d3591016119d0565b9a01998a5201611923565b9d61010081019e8f525116828b511690611ad8565b9490935116965160020b915116936040519260a084019084821067ffffffffffffffff831117610975576001600160a01b03916040521683526001600160a01b0360208401941684526040830196875260608301918252608083019485528551151594855f14610989576401000276ad915b895191600160ff1b83146108cf57604051916060830198838a1067ffffffffffffffff8b1117610975578f99604052835260208301935f03845260408301946001600160a01b0316855251946040519a8b988998633cf3645360e21b8a52516001600160a01b031660048a0152516001600160a01b031660248901525162ffffff1660448801525160020b6064870152516001600160a01b0316608486015251151560a48501525160c4840152516001600160a01b031660e4830152610104820161012090526101248201610626916118ab565b03815a6020945f91f1918215610271575f92610941575b506001600160a01b03885116863b156102655760405190632961046560e21b825260048201525f81602481838b5af1801561027157610931575b50805115610928578160801d5b600f0b5f8112156108e3576f7fffffffffffffffffffffffffffffff1981146108cf576fffffffffffffffffffffffffffffffff905f03169251830361088a57511561088157600f0b955b5f87600f0b131561083c57516001600160a01b031692836107de5750604051630476982d60e21b8152925060209083906004908290875af1908115610271576001600160a01b039283926107af575b505b511692511690803b15610265575f92836064926fffffffffffffffffffffffffffffffff6040519788968795630b0d9c0960e01b8752600487015260248601521660448401525af180156102715761079f575b61079b60405161078460208261197e565b5f81526040519182916020835260208301906118ab565b0390f35b5f6107a99161197e565b80610773565b6107d09060203d6020116107d7575b6107c8818361197e565b810190611a06565b508661071e565b503d6107be565b6107e89293611a4f565b604051630476982d60e21b8152906020826004815f875af1908115610271576001600160a01b0392839261081d575b50610720565b6108359060203d6020116107d7576107c8818361197e565b5086610817565b60405162461bcd60e51b815260206004820152601960248201527f56343a204e65676174697665206f75747075742064656c7461000000000000006044820152606490fd5b60801d956106cf565b60405162461bcd60e51b815260206004820152601b60248201527f56343a20616d6f756e74546f50617920213d20616d6f756e74496e00000000006044820152606490fd5b634e487b7160e01b5f52601160045260245ffd5b60405162461bcd60e51b815260206004820152601860248201527f56343a20506f73697469766520696e7075742064656c746100000000000000006044820152606490fd5b81600f0b610684565b5f61093b9161197e565b88610677565b9091506020813d60201161096d575b8161095d6020938361197e565b810103126102655751908861063d565b3d9150610950565b634e487b7160e01b5f52604160045260245ffd5b73fffd8963efd1fc6a506488495d951d5263988d2591610552565b60405162461bcd60e51b815260206004820152603460248201527f556e697377617056345377617043616c6c6261636b3a204d73672e73656e646560448201527f72206973206e6f7420506f6f6c4d616e616765720000000000000000000000006064820152608490fd5b34610265575f3660031901126102655760206040516001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000168152f35b34610265576001600160a01b0380610a7160206100ac61009e36611874565b03816001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000165afa938415610271575f94610bac575b506001600160a01b0384168033149081610ba2575b5015610b4557851015610b3d5750915b8203610ae15761001a92611a4f565b60405162461bcd60e51b815260206004820152602e60248201527f50616e63616b6556335377617043616c6c6261636b3a20616d6f756e74546f5060448201526d30bc90109e9030b6b7bab73a24b760911b6064820152608490fd5b905091610ad2565b60405162461bcd60e51b815260206004820152602f60248201527f50616e63616b6556335377617043616c6c6261636b3a204d73672e73656e646560448201526e1c881a5cc81b9bdd0818481c1bdbdb608a1b6064820152608490fd5b9050151587610ac2565b9093506020813d602011610be0575b81610bc86020938361197e565b8101031261026557610bd9906119a0565b9286610aad565b3d9150610bbb565b6020366003190112610265576004359067ffffffffffffffff82116102655781600401908236039060a0600319830112610265573033036118045750426084840135106117bf575f915f9060448501946001600160a01b03610c49876118cf565b166117ac573331915b610c5c81806118e3565b5f905f925f5b8281106117655750505061173e575b6116e4575b5f602483013595602219018612159583019260048401359267ffffffffffffffff84119160248601968560051b36038813945b610cb384806118e3565b90508110156115fd578a81610cdb610ccb87806118e3565b6001600160f81b03199391611a43565b3516906102655785610265578661026557878210156115e957610d11610d0a6024600585901b8c01018c6118e3565b36916119d0565b90600160f81b811015806115db575b1561159657600160f81b81148015611589575b6110a8575b600360f81b8114610fce575b600160fa1b8114610eed575b600560f81b8114610dfe575b600360f91b14610d70575b50600101610ca9565b805181019060208183031261026557602080610d8e93019101611aa9565b51907f00000000000000000000000000000000000000000000000000000000000000006001600160a01b0316803b15610265575f90600460405180958193630d0e30db60e41b83525af191821561027157600192610dee575b5090610d67565b5f610df89161197e565b8e610de7565b81518201602083820312610265576020610e1c910160208401611aa9565b7f0000000000000000000000000000000000000000000000000000000000000000610e508e610e4b3384611a15565b611916565b91518210610ea9576001600160a01b031690813b15610265575f91602483926040519485938492632e1a7d4d60e01b845260048401525af1801561027157610e99575b50610d5c565b5f610ea39161197e565b8f610e93565b606460405162461bcd60e51b815260206004820152602060248201527f536c697070616765436865636b3a20496e73756666696369656e7420574554486044820152fd5b8151820160208382031261026557610f188f916020610f10910160208601611aa9565b913331611916565b90518110610f89576001600160a01b037f000000000000000000000000000000000000000000000000000000000000000016803b15610265575f90600460405180948193630d0e30db60e41b83525af1801561027157610f79575b50610d50565b5f610f839161197e565b8f610f73565b60405162461bcd60e51b815260206004820152601f60248201527f536c697070616765436865636b3a20496e73756666696369656e7420455448006044820152606490fd5b6040516348c8949160e01b8152602060048201525f8180610ff260248201876118ab565b0381836001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000165af1801561027157611032575b50610d44565b3d805f833e611041818361197e565b8101906020818303126102655780519067ffffffffffffffff8211610265570181601f82011215610265578051611077816119b4565b92611085604051948561197e565b81845260208284010111610265575f928160208094018483015e0101528f61102c565b60c0828051810103126102655760405160c0810181811067ffffffffffffffff82111761097557604052602083015181526110e5604084016119a0565b602082019081526110f8606085016119a0565b906040830191825261110c608086016119a0565b916060840192835260a08601519360ff60f81b85169485810361026557608082015260c08701519062ffffff821682036102655760a081019182528561143d57600492939495506111746001600160a01b038651166001600160a01b03885116835191611a4f565b60606001600160a01b0387511660405194858092630240bc6b60e21b82525afa928315610271575f905f946113d8575b506dffffffffffffffffffffffffffff8091169316926001600160a01b038651166001600160a01b03865116115f146113cd5762ffffff9093915b519251169180156113745783159283158061136b575b1561131557606490046127100361271081116108cf5761122391600a61121c920490611b15565b9182611b15565b916103e884029384046103e81417156108cf5782018092116108cf578115611301576001600160a01b0391829104935116915116115f146112f2576001600160a01b035f91925b516040519116602061127c818461197e565b5f8352601f198101903690840137803b15610265576112cd935f80946040519687958694859363022c0d9f60e01b8552600485015260248401523360448401526080606484015260848301906118ab565b03925af18015610271576112e2575b50610d38565b5f6112ec9161197e565b8f6112dc565b6001600160a01b035f9261126a565b634e487b7160e01b5f52601260045260245ffd5b60405162461bcd60e51b815260206004820152602860248201527f556e697377617056324c6962726172793a20494e53554646494349454e545f4c604482015267495155494449545960c01b6064820152608490fd5b508215156111f5565b60405162461bcd60e51b815260206004820152602b60248201527f556e697377617056324c6962726172793a20494e53554646494349454e545f4960448201526a1394155517d05353d5539560aa1b6064820152608490fd5b9062ffffff906111df565b9350506060833d8211611435575b816113f36060938361197e565b810103126102655761140483611afa565b604061141260208601611afa565b94015163ffffffff811603610265576dffffffffffffffffffffffffffff6111a4565b3d91506113e6565b929491939291600160f81b03611544575f6040946114e56001600160a01b03808099511692511691828110928385146115245762ffffff8a6401000276ad995b51169751965116908951926020840152898301528560608301526080820152608081526114ab60a08261197e565b875198899788968795630251596160e31b87523360048801526024870152604486015216606484015260a0608484015260a48301906118ab565b03925af18015610271576114f95750610d38565b604090813d811161151d575b61150f818361197e565b81010312610265578f6112dc565b503d611505565b62ffffff8a73fffd8963efd1fc6a506488495d951d5263988d259961147d565b60405162461bcd60e51b815260206004820152601460248201527f496e76616c696420706f6f6c2076617269616e740000000000000000000000006044820152606490fd5b50600160f91b8114610d33565b60405162461bcd60e51b815260206004820152600f60248201527f496e76616c696420636f6d6d616e6400000000000000000000000000000000006044820152606490fd5b50600360f91b811115610d20565b634e487b7160e01b5f52603260045260245ffd5b508b6001600160a01b03611610826118cf565b166116cc575033315b818111156116875760649161162d91611916565b9101351161163757005b60405162461bcd60e51b815260206004820152602260248201527f536c697070616765436865636b3a20496e73756666696369656e74206f7574706044820152611d5d60f21b6064820152608490fd5b60405162461bcd60e51b815260206004820152601c60248201527f42616420537761703a204e6f20616d6f756e74207265636569766564000000006044820152606490fd5b6116df906116da33916118cf565b611a15565b611619565b92506116ef866118cf565b7f0000000000000000000000000000000000000000000000000000000000000000906001600160a01b038083169116145f1461172d57508192610c76565b611738903390611a15565b92610c76565b95506001600160a01b03611751886118cf565b1661175d578295610c71565b333195610c71565b6001600160f81b0319611779828585611a43565b3516600160fa1b81146117a3575b600560f81b1461179a575b600101610c62565b60019450611792565b60019450611787565b6117b9336116da886118cf565b91610c52565b60405162461bcd60e51b815260206004820152601160248201527f446561646c696e653a20457870697265640000000000000000000000000000006044820152606490fd5b62461bcd60e51b815260206004820152601560248201527f4f6e6c792063616c6c61626c652062792073656c6600000000000000000000006044820152606490fd5b9181601f840112156102655782359167ffffffffffffffff8311610265576020838186019501011161026557565b60606003198201126102655760043591602435916044359067ffffffffffffffff8211610265576118a791600401611846565b9091565b805180835260209291819084018484015e5f828201840152601f01601f1916010190565b356001600160a01b03811681036102655790565b903590601e1981360301821215610265570180359067ffffffffffffffff82116102655760200191813603831361026557565b919082039182116108cf57565b35906001600160a01b038216820361026557565b359062ffffff8216820361026557565b91908260809103126102655761195c82611923565b9161196960208201611923565b9161197b606060408401359301611937565b90565b90601f8019910116810190811067ffffffffffffffff82111761097557604052565b51906001600160a01b038216820361026557565b67ffffffffffffffff811161097557601f01601f191660200190565b9291926119dc826119b4565b916119ea604051938461197e565b829481845281830111610265578281602093845f960137010152565b90816020910312610265575190565b602460106020939284936014526f70a082310000000000000000000000005f525afa601f3d11166020510290565b908210156115e9570190565b91906014526034526fa9059cbb0000000000000000000000005f5260205f6044601082855af1908160015f51141615611a8b575b50505f603452565b3b153d171015611a9c575f80611a83565b6390b8ec185f526004601cfd5b9190826020910312610265576040516020810181811067ffffffffffffffff8211176109755760405291518252565b9190916001600160a01b0383166001600160a01b038216105f1461197b579190565b51906dffffffffffffffffffffffffffff8216820361026557565b818102929181159184041417156108cf5756fea2646970667358221220bab2ea1ae28003dec88e9ba177c33b35539e1af292171878364d0856592c0e7164736f6c634300081e0033";

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn test_unauthorized_access_should_fail() {
      // ATTACK VECTOR 1: Steal ETH from the delegated EOA

      // Attacker: Bob
      // Victim: Alice

      // Attach the ZeusSwapDelegator contract to Alice
      // Bob will try to steal all the ETH from Alice by swapping ETH for USDT on a V4 Pool
      // Bob will do that by calling the public zSwap function on Alice address

      let chain_id = 1;
      let ctx = ZeusCtx::new();

      let client = ctx.get_client(chain_id).await.unwrap();
      let block = client.get_block(BlockId::latest()).await.unwrap().unwrap();
      let mut fork_factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);

      let ten_eth = NumericValue::parse_to_wei("10", 18);

      let currency_in = Currency::from(NativeCurrency::from(chain_id));
      let currency_out = Currency::from(ERC20Token::usdt());

      let alice = DummyAccount::new(AccountType::EOA, ten_eth.wei());
      let bob = DummyAccount::new(AccountType::EOA, U256::ZERO);

      fork_factory.insert_dummy_account(alice.clone());
      fork_factory.insert_dummy_account(bob.clone());

      let fork_db = fork_factory.new_sandbox_fork();
      let mut evm = new_evm(chain_id.into(), Some(&block), fork_db);

      let alice_eth_before = evm.balance(alice.address).unwrap().data;
      eprintln!("Alice's ETH Balance Before: {}", alice_eth_before);

      let bob_usdt_before =
         simulate::erc20_balance(&mut evm, currency_out.address(), bob.address).unwrap();
      eprintln!("Bob's USDT Balance Before: {}", bob_usdt_before);

      // Attach the contract to Alice
      let mut alice_nonce = 0;
      let delegate_addr =
         deploy_zeus_delegator(&mut evm, alice.address, alice_nonce, chain_id).unwrap();

      alice_nonce += 1;

      let auth = Authorization {
         chain_id: U256::from(chain_id),
         address: delegate_addr,
         nonce: alice_nonce + 1,
      };

      let signature = alice.key.sign_hash_sync(&auth.signature_hash()).unwrap();
      let signed_authorization = auth.into_signed(signature);

      evm.cfg.disable_nonce_check = true;

      evm.tx.authorization_list = vec![Either::Left(signed_authorization)];
      evm.tx.data = Bytes::default();
      evm.tx.kind = TxKind::Call(alice.address);
      evm.tx.value = U256::ZERO;
      evm.tx.nonce = alice_nonce;
      evm.tx.tx_type = 4;

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      if !res.is_success() {
         let err = revert_msg(&output);
         eprintln!("Call Reverted: {}", err);
         eprintln!("Output: {:?}", output);
         eprintln!("Gas Used: {}", res.gas_used());
         panic!("Attaching the contract to Alice failed");
      }

      let code = evm.load_account_code(alice.address).unwrap().data;
      assert!(!code.is_empty());

      // Now Bob will try to steal the ETH from Alice by swapping ETH for USDT on a V4 Pool

      let eth_usdt = UniswapV4Pool::eth_usdt();

      let mut commands = Vec::new();
      let mut inputs = Vec::new();

      let v4_swap_params = zeus_abi::ZeusDelegate::V4SwapArgs {
         currencyIn: currency_in.address(),
         currencyOut: currency_out.address(),
         amountIn: ten_eth.wei(),
         fee: eth_usdt.fee.fee_u24(),
         tickSpacing: eth_usdt.fee.tick_spacing(),
         zeroForOne: eth_usdt.zero_for_one(&currency_in),
         hooks: Address::ZERO,
         hookData: Bytes::default(),
         recipient: bob.address,
      }
      .abi_encode()
      .into();

      commands.push(Commands::V4_SWAP as u8);
      inputs.push(v4_swap_params);

      let call_data = zeus_abi::encode_z_swap(
         commands.into(),
         inputs,
         currency_out.address(),
         U256::ZERO,
         U256::MAX,
      );

      evm.tx.data = call_data.clone();
      evm.tx.caller = bob.address;
      evm.tx.value = U256::ZERO;
      evm.tx.kind = TxKind::Call(alice.address);
      evm.tx.nonce = 0;
      evm.tx.tx_type = 4;

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      if !res.is_success() {
         let err = revert_msg(&output);
         let expected = "revert: Only callable by self";

         if expected != err {
            panic!("Call reverted but with unexpected error: {}", err);
         } else {
            eprintln!("Bob's attack failed, Reason {}", err);
            return;
         }
      }

      evm.cfg.disable_nonce_check = true;

      let alice_eth_balance = evm.balance(alice.address).unwrap().data;
      eprintln!("Alice's ETH Balance After: {}", alice_eth_balance);

      let bob_usdt_balance =
         simulate::erc20_balance(&mut evm, currency_out.address(), bob.address).unwrap();

      eprintln!("Bob's USDT Balance After: {}", bob_usdt_balance);

      assert_eq!(alice_eth_balance, U256::ZERO);
      assert!(bob_usdt_balance > U256::ZERO);
      panic!("Bob's attack successful");
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn v2_swap_erc20_to_erc20_mainnet() {
      let chain_id = 1;

      let pool: AnyUniswapPool = UniswapV2Pool::weth_uni().into();
      let currency_in = Currency::from(ERC20Token::weth());
      let currency_out = pool.quote_currency().clone();
      let amount_in = NumericValue::parse_to_wei("10", currency_in.decimals());

      let swap_on_v2 = true;
      let swap_on_v3 = true;
      let swap_on_v4 = true;
      let max_hops = 2;
      let max_routes = 1;
      let with_split_routing = true;

      test_swap_eip7702(
         chain_id,
         amount_in,
         currency_in,
         currency_out,
         swap_on_v2,
         swap_on_v3,
         swap_on_v4,
         max_hops,
         max_routes,
         with_split_routing,
         vec![pool],
      )
      .await
      .unwrap();
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn v4_single_swap_mainnet() {
      let chain_id = 1;

      let currency_in = Currency::from(NativeCurrency::from(chain_id));
      let currency_out = Currency::from(ERC20Token::usdt());
      let amount_in = NumericValue::parse_to_wei("1", currency_in.decimals());

      let swap_on_v2 = false;
      let swap_on_v3 = false;
      let swap_on_v4 = true;
      let max_hops = 2;
      let max_routes = 1;
      let with_split_routing = false;

      test_swap_eip7702(
         chain_id,
         amount_in,
         currency_in,
         currency_out,
         swap_on_v2,
         swap_on_v3,
         swap_on_v4,
         max_hops,
         max_routes,
         with_split_routing,
         vec![],
      )
      .await
      .unwrap();
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn big_swap_from_usdt_to_eth_mainnet() {
      let chain_id = 1;

      let currency_in = Currency::from(ERC20Token::usdt());
      let currency_out = Currency::from(NativeCurrency::from(chain_id));
      let amount_in = NumericValue::parse_to_wei("1000000", currency_in.decimals());

      let swap_on_v2 = true;
      let swap_on_v3 = true;
      let swap_on_v4 = true;
      let max_hops = 10;
      let max_routes = 10;
      let with_split_routing = true;

      test_swap_eip7702(
         chain_id,
         amount_in,
         currency_in,
         currency_out,
         swap_on_v2,
         swap_on_v3,
         swap_on_v4,
         max_hops,
         max_routes,
         with_split_routing,
         vec![],
      )
      .await
      .unwrap();
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn big_swap_from_usdt_to_weth_mainnet() {
      let chain_id = 1;

      let currency_in = Currency::from(ERC20Token::usdt());
      let currency_out = Currency::from(ERC20Token::weth());
      let amount_in = NumericValue::parse_to_wei("1000000", currency_in.decimals());

      let swap_on_v2 = true;
      let swap_on_v3 = true;
      let swap_on_v4 = true;
      let max_hops = 10;
      let max_routes = 10;
      let with_split_routing = true;

      test_swap_eip7702(
         chain_id,
         amount_in,
         currency_in,
         currency_out,
         swap_on_v2,
         swap_on_v3,
         swap_on_v4,
         max_hops,
         max_routes,
         with_split_routing,
         vec![],
      )
      .await
      .unwrap();
   }

   async fn test_swap_eip7702(
      chain: u64,
      amount_in: NumericValue,
      currency_in: Currency,
      currency_out: Currency,
      swap_on_v2: bool,
      swap_on_v3: bool,
      swap_on_v4: bool,
      max_hops: usize,
      max_routes: usize,
      with_split_routing: bool,
      given_pools: Vec<AnyUniswapPool>,
   ) -> Result<(), anyhow::Error> {
      let ctx = ZeusCtx::new();

      let pools = if given_pools.is_empty() {
         let relevant_pools = get_relevant_pools(
            ctx.clone(),
            swap_on_v2,
            swap_on_v3,
            swap_on_v4,
            &currency_in,
            &currency_out,
         );
         relevant_pools
      } else {
         given_pools
      };

      let pool_manager = ctx.pool_manager();
      let updated_pools = pool_manager.update_state_for_pools(ctx.clone(), chain, pools).await?;

      let eth = Currency::from(NativeCurrency::from(chain));
      let eth_price = ctx.get_currency_price(&eth);
      let currency_out_price = ctx.get_currency_price(&currency_out);
      let base_fee = BaseFee::default();
      let priority_fee = NumericValue::parse_to_gwei("1");

      let quote = if with_split_routing {
         get_quote_with_split_routing(
            ctx.clone(),
            amount_in.clone(),
            currency_in.clone(),
            currency_out.clone(),
            updated_pools,
            eth_price.clone(),
            currency_out_price.clone(),
            base_fee.next,
            priority_fee.wei(),
            max_hops,
            max_routes,
         )
      } else {
         get_quote(
            ctx.clone(),
            amount_in.clone(),
            currency_in.clone(),
            currency_out.clone(),
            updated_pools,
            eth_price.clone(),
            currency_out_price.clone(),
            base_fee.next,
            priority_fee.wei(),
            max_hops,
         )
      };

      let slippage = 0.5;
      let swap_steps = quote.swap_steps;
      let amount_out = quote.amount_out;
      let min_amount_out = amount_out.calc_slippage(slippage, currency_out.decimals());

      eprintln!(
         "Quote {} {} For {} {}",
         amount_in.format_abbreviated(),
         currency_in.symbol(),
         currency_out.symbol(),
         amount_out.format_abbreviated()
      );
      eprintln!("Swap Steps Length: {}", swap_steps.len());

      for swap in &swap_steps {
         eprintln!(
            "Swap Step: {} (Wei: {}) {} -> {} (Wei: {}) {} {} ({})",
            swap.amount_in.format_abbreviated(),
            swap.amount_in.wei(),
            swap.currency_in.symbol(),
            swap.amount_out.format_abbreviated(),
            swap.amount_out.wei(),
            swap.currency_out.symbol(),
            swap.pool.dex_kind().as_str(),
            swap.pool.fee().fee()
         );
      }

      let client = ctx.get_client(chain).await?;

      let eth_balance = if currency_in.is_native() {
         amount_in.wei()
      } else {
         U256::ZERO
      };

      eprintln!("Alice ETH Balance: {}", eth_balance);
      let alice = DummyAccount::new(AccountType::EOA, eth_balance);
      let _signer = SecureSigner::from(alice.key.clone());

      let block = client.get_block(BlockId::latest()).await.unwrap();
      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain, None, None);
      factory.insert_dummy_account(alice.clone());

      if currency_in.is_erc20() {
         factory.give_token(
            alice.address,
            currency_in.address(),
            amount_in.wei(),
         )?;
      }

      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain.into(), block.as_ref(), fork_db);
      evm.cfg.disable_nonce_check = false;

      let delegate_addr = deploy_zeus_delegator(&mut evm, alice.address, 0, chain)?;

      let swap_params = encode_swap_delegate(
         chain,
         swap_steps,
         amount_in.wei(),
         min_amount_out.wei(),
         slippage,
         currency_in.clone(),
         currency_out.clone(),
         alice.address,
      )
      .await?;

      eprintln!("Commands: {}", swap_params.commands);

      let authorization = Authorization {
         chain_id: U256::from(chain),
         address: delegate_addr,
         nonce: 2,
      };

      let signature = alice.key.sign_hash_sync(&authorization.signature_hash())?;
      let signed_authorization = authorization.into_signed(signature);

      evm.tx.caller = alice.address;
      evm.tx.data = swap_params.call_data.clone();
      evm.tx.authorization_list = vec![Either::Left(signed_authorization)];
      evm.tx.value = swap_params.value.clone();
      evm.tx.kind = TxKind::Call(alice.address);
      evm.tx.nonce = 1;
      evm.tx.chain_id = Some(chain);
      evm.tx.tx_type = 4;

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      if !res.is_success() {
         let err = revert_msg(&output);
         eprintln!("Call Reverted: {}", err);
         eprintln!("Output: {:?}", output);
         eprintln!("Gas Used: {}", res.gas_used());
         panic!("Delegate Call Failed");
      }

      eprintln!("Delegate Call Successful");
      eprintln!("Gas Used: {}", res.gas_used());

      let code = evm.load_account_code(alice.address).unwrap().data;
      assert!(!code.is_empty());

      evm.cfg.disable_nonce_check = true;

      let currency_out_balance = if currency_out.is_erc20() {
         simulate::erc20_balance(&mut evm, currency_out.address(), alice.address).unwrap()
      } else {
         let state = evm.balance(alice.address).unwrap();
         state.data
      };

      if currency_out_balance < min_amount_out.wei() {
         panic!(
            "TooLittleReceived, expected {} got {}",
            min_amount_out.wei(),
            currency_out_balance
         );
      }

      let balance = NumericValue::format_wei(currency_out_balance, currency_out.decimals());

      eprintln!(
         "{} Quote Amount: {}",
         currency_out.symbol(),
         amount_out.format_abbreviated()
      );

      eprintln!(
         "{} Got from Swap: {}",
         currency_out.symbol(),
         balance.format_abbreviated()
      );

      let auth = Authorization {
         chain_id: U256::from(chain),
         address: Address::ZERO,
         nonce: 4,
      };

      let signature = alice.key.sign_hash_sync(&auth.signature_hash()).unwrap();
      let signed_authorization = auth.into_signed(signature);

      evm.tx.authorization_list = vec![Either::Left(signed_authorization)];
      evm.tx.data = Bytes::default();
      evm.tx.kind = TxKind::Call(alice.address);
      evm.tx.value = U256::ZERO;
      evm.tx.nonce = 3;
      evm.tx.chain_id = Some(chain);
      evm.tx.tx_type = 4;

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      if !res.is_success() {
         let err = revert_msg(&output);
         eprintln!("Call Reverted: {}", err);
         eprintln!("Output: {:?}", output);
         panic!("Restoring Alice Failed");
      }

      let code = evm.load_account_code(alice.address).unwrap().data;
      assert!(code.is_empty());

      Ok(())
   }

   fn deploy_zeus_delegator(
      evm: &mut Evm2<ForkDB>,
      caller: Address,
      nonce: u64,
      chain: u64,
   ) -> Result<Address, anyhow::Error> {
      let weth = address_book::weth(chain)?;
      let v4_pool_manager = address_book::uniswap_v4_pool_manager(chain)?;
      let uni_v3_factory = address_book::uniswap_v3_factory(chain)?;
      let pancake_v3_factory = address_book::pancakeswap_v3_factory(chain)?;

      let deploy_params = zeus_abi::ZeusDelegate::DeployParams {
         weth,
         v4PoolManager: v4_pool_manager,
         uniswapV3Factory: uni_v3_factory,
         pancakeSwapV3Factory: pancake_v3_factory,
      }
      .abi_encode();

      let code = Bytes::from_str(ZEUS_DELEGATE_CODE)?;
      let bytecode = [code.as_ref(), &deploy_params].concat();

      evm.tx.caller = caller;
      evm.tx.data = bytecode.into();
      evm.tx.kind = TxKind::Create;
      evm.tx.value = U256::ZERO;
      evm.tx.nonce = nonce;

      let res = evm.transact_commit(evm.tx.clone())?;
      eprintln!("Gas Used for Deploy: {}", res.gas_used());

      let address = match res {
         ExecutionResult::Success { output, .. } => output.address().cloned(),
         _ => None,
      };

      Ok(address.unwrap())
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn test_deploy_delegator() {
      let ctx = ZeusCtx::new();

      let chain = 1;

      let client = ctx.get_client(chain).await.unwrap();
      let block = client.get_block(BlockId::latest()).await.unwrap().unwrap();
      let fork_factory = ForkFactory::new_sandbox_factory(client.clone(), chain, None, None);
      let fork_db = fork_factory.new_sandbox_fork();
      let mut evm = new_evm(chain.into(), Some(&block), fork_db);

      let weth = address_book::weth(chain).unwrap();
      let v4_pool_manager = address_book::uniswap_v4_pool_manager(chain).unwrap();
      let uni_v3_factory = address_book::uniswap_v3_factory(chain).unwrap();
      let pancake_v3_factory = address_book::pancakeswap_v3_factory(chain).unwrap();

      let deploy_params = zeus_abi::ZeusDelegate::DeployParams {
         weth,
         v4PoolManager: v4_pool_manager,
         uniswapV3Factory: uni_v3_factory,
         pancakeSwapV3Factory: pancake_v3_factory,
      }
      .abi_encode();

      let code = Bytes::from_str(ZEUS_DELEGATE_CODE).unwrap();
      let bytecode = [code.as_ref(), &deploy_params].concat();

      let alice = DummyAccount::new(AccountType::EOA, U256::ZERO);

      evm.tx.caller = alice.address;
      evm.tx.data = bytecode.into();
      evm.tx.kind = TxKind::Create;
      evm.tx.value = U256::ZERO;

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      eprintln!("Gas Used for Deploy: {}", res.gas_used());

      let address = match res {
         ExecutionResult::Success { output, .. } => output.address().cloned().unwrap(),
         _ => panic!("Failed to deploy router"),
      };

      eprintln!("Router Deployed At Address: {}", address);

      evm.tx.data = zeus_abi::ZeusDelegate::WETHCall {}.abi_encode().into();
      evm.tx.kind = TxKind::Call(address);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      let weth_address =
         zeus_abi::ZeusDelegate::WETHCall::abi_decode_returns(output.as_ref()).unwrap();
      assert_eq!(weth_address, weth);

      evm.tx.data = zeus_abi::ZeusDelegate::V4_POOL_MANAGERCall {}.abi_encode().into();
      evm.tx.kind = TxKind::Call(address);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      let v4_pool_manager_address =
         zeus_abi::ZeusDelegate::V4_POOL_MANAGERCall::abi_decode_returns(output.as_ref()).unwrap();
      assert_eq!(v4_pool_manager_address, v4_pool_manager);

      evm.tx.data = zeus_abi::ZeusDelegate::UNISWAP_V3_FACTORYCall {}.abi_encode().into();
      evm.tx.kind = TxKind::Call(address);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      let uni_v3_factory_address =
         zeus_abi::ZeusDelegate::UNISWAP_V3_FACTORYCall::abi_decode_returns(output.as_ref())
            .unwrap();
      assert_eq!(uni_v3_factory_address, uni_v3_factory);

      evm.tx.data = zeus_abi::ZeusDelegate::PANCAKE_SWAP_V3_FACTORYCall {}.abi_encode().into();
      evm.tx.kind = TxKind::Call(address);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      let pancake_v3_factory_address =
         zeus_abi::ZeusDelegate::PANCAKE_SWAP_V3_FACTORYCall::abi_decode_returns(output.as_ref())
            .unwrap();
      assert_eq!(pancake_v3_factory_address, pancake_v3_factory);
   }
}
