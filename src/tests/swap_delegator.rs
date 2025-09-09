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

   const ZEUS_DELEGATE_CODE: &str = "0x6101006040523461013357604051601f611d7338819003918201601f19168301916001600160401b0383118484101761011f578084926080946040528339810103126101335760405190608082016001600160401b0381118382101761011f5760405261006b81610137565b80835261007a60208301610137565b906020840191825260606100a28161009460408701610137565b956040880196875201610137565b94019384526001600160a01b039081166080529051811660a0529051811660c05290511660e052604051611c27908161014c82396080518181816103220152610c7d015260a0518181816102df01528181610375015261119c015260c051818181610103015261029c015260e051818181610a1d0152610a6c0152f35b634e487b7160e01b5f52604160045260245ffd5b5f80fd5b51906001600160a01b03821682036101335756fe608080604052600436101561001c575b50361561001a575f80fd5b005b5f3560e01c90816301a3f1e814610bd75750806323a69e7514610a41578063695033f3146109fe57806391dd734614610346578063ad5c464814610303578063e34e7283146102c0578063f73e5aab1461027d5763fa461e3314610080575f61000f565b34610266576001600160a01b03806100f760206100ad61009f366118d6565b8195939594929401906119a9565b604051630b4c774160e11b8152939098166001600160a01b03818116600486015299909216988916602484015262ffffff9097166044830152969094909190859081906064820190565b03816001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000165afa938415610272575f94610232575b506001600160a01b0384168033149081610228575b50156101cb578510156101c35750915b82036101675761001a92611ac2565b60405162461bcd60e51b815260206004820152602e60248201527f556e697377617056335377617043616c6c6261636b3a20616d6f756e74546f5060448201526d30bc90109e9030b6b7bab73a24b760911b6064820152608490fd5b905091610158565b60405162461bcd60e51b815260206004820152602f60248201527f556e697377617056335377617043616c6c6261636b3a204d73672e73656e646560448201526e1c881a5cc81b9bdd0818481c1bdbdb608a1b6064820152608490fd5b905015155f610148565b9093506020813d60201161026a575b8161024e602093836119fd565b810103126102665761025f90611a1f565b925f610133565b5f80fd5b3d9150610241565b6040513d5f823e3d90fd5b34610266575f3660031901126102665760206040516001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000168152f35b34610266575f3660031901126102665760206040516001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000168152f35b34610266575f3660031901126102665760206040516001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000168152f35b60203660031901126102665760043567ffffffffffffffff8111610266576103729036906004016118a8565b907f0000000000000000000000000000000000000000000000000000000000000000916001600160a01b03831690813303610993578201916020818403126102665780359067ffffffffffffffff82116102665701926101208484031261026657604051936103e0856119e0565b6103e981611985565b85526103f760208201611985565b906020860191825260408601906040810135825261041760608201611999565b90606088019182526080810135918260020b8303610266576080890192835260a08201359182151583036102665760a08a0192835261045860c08201611985565b9160c08b0192835260e082013567ffffffffffffffff8111610266578201928a601f85011215610266576001600160a01b036104cf816104ba8f9e8f989760e06104af610100938b602062ffffff9d359101611a4f565b9a01998a5201611985565b9d61010081019e8f525116828b511690611ba1565b9490935116965160020b915116936040519260a084019084821067ffffffffffffffff831117610964576001600160a01b03916040521683526001600160a01b0360208401941684526040830196875260608301918252608083019485528551151594855f14610978576401000276ad915b895191600160ff1b83146108be57604051916060830198838a1067ffffffffffffffff8b1117610964578f99604052835260208301935f03845260408301946001600160a01b0316855251946040519a8b988998633cf3645360e21b8a52516001600160a01b031660048a0152516001600160a01b031660248901525162ffffff1660448801525160020b6064870152516001600160a01b0316608486015251151560a48501525160c4840152516001600160a01b031660e48301526101048201610120905261012482016106159161190d565b03815a6020945f91f1918215610272575f92610930575b506001600160a01b03885116863b156102665760405190632961046560e21b825260048201525f81602481838b5af1801561027257610920575b50805115610917578160801d5b600f0b5f8112156108d2576f7fffffffffffffffffffffffffffffff1981146108be576fffffffffffffffffffffffffffffffff905f03169251830361087957511561087057600f0b955b5f87600f0b131561082b57516001600160a01b031692836107cd5750604051630476982d60e21b8152925060209083906004908290875af1908115610272576001600160a01b0392839261079e575b505b511692511690803b15610266575f92836064926fffffffffffffffffffffffffffffffff6040519788968795630b0d9c0960e01b8752600487015260248601521660448401525af180156102725761078e575b61078a6040516107736020826119fd565b5f815260405191829160208352602083019061190d565b0390f35b5f610798916119fd565b80610762565b6107bf9060203d6020116107c6575b6107b781836119fd565b810190611a85565b508661070d565b503d6107ad565b6107d79293611ac2565b604051630476982d60e21b8152906020826004815f875af1908115610272576001600160a01b0392839261080c575b5061070f565b6108249060203d6020116107c6576107b781836119fd565b5086610806565b60405162461bcd60e51b815260206004820152601960248201527f56343a204e65676174697665206f75747075742064656c7461000000000000006044820152606490fd5b60801d956106be565b60405162461bcd60e51b815260206004820152601b60248201527f56343a20616d6f756e74546f50617920213d20616d6f756e74496e00000000006044820152606490fd5b634e487b7160e01b5f52601160045260245ffd5b60405162461bcd60e51b815260206004820152601860248201527f56343a20506f73697469766520696e7075742064656c746100000000000000006044820152606490fd5b81600f0b610673565b5f61092a916119fd565b88610666565b9091506020813d60201161095c575b8161094c602093836119fd565b810103126102665751908861062c565b3d915061093f565b634e487b7160e01b5f52604160045260245ffd5b73fffd8963efd1fc6a506488495d951d5263988d2591610541565b60405162461bcd60e51b815260206004820152603460248201527f556e697377617056345377617043616c6c6261636b3a204d73672e73656e646560448201527f72206973206e6f7420506f6f6c4d616e616765720000000000000000000000006064820152608490fd5b34610266575f3660031901126102665760206040516001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000168152f35b34610266576001600160a01b0380610a6060206100ad61009f366118d6565b03816001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000165afa938415610272575f94610b9b575b506001600160a01b0384168033149081610b91575b5015610b3457851015610b2c5750915b8203610ad05761001a92611ac2565b60405162461bcd60e51b815260206004820152602e60248201527f50616e63616b6556335377617043616c6c6261636b3a20616d6f756e74546f5060448201526d30bc90109e9030b6b7bab73a24b760911b6064820152608490fd5b905091610ac1565b60405162461bcd60e51b815260206004820152602f60248201527f50616e63616b6556335377617043616c6c6261636b3a204d73672e73656e646560448201526e1c881a5cc81b9bdd0818481c1bdbdb608a1b6064820152608490fd5b9050151587610ab1565b9093506020813d602011610bcf575b81610bb7602093836119fd565b8101031261026657610bc890611a1f565b9286610a9c565b3d9150610baa565b6020366003190112610266576004359067ffffffffffffffff821161026657816004019180360391608060031984011261026657303303611866575060448101916001600160a01b03610c2984611931565b1661185057610c3b3331945b80611945565b939091602484013590602219018112156102665783019160048301359267ffffffffffffffff84116102665760248101918460051b36038313610266573331957f000000000000000000000000000000000000000000000000000000000000000098610ca7338b611a94565b955f5b8a81101561176a57848101357fff00000000000000000000000000000000000000000000000000000000000000168982101561175657610cfd610cf66024600585901b8a01018a611945565b3691611a4f565b90600160f81b81101580611748575b1561170357600160f81b811480156116f6575b61121a575b600360f81b8114610f5b575b600160fa1b8114610e9c575b600560f81b8114610dca575b600360f91b14610d5c575b50600101610caa565b805181019060208183031261026657602080610d7a93019101611b72565b51906001600160a01b038d16803b15610266575f90600460405180958193630d0e30db60e41b83525af191821561027257600192610dba575b5090610d53565b5f610dc4916119fd565b8d610db3565b8151820160208382031261026657610dfe8f91610df9610df160208f930160208801611b72565b933390611a94565b611978565b90518110610e58576001600160a01b038f1690813b15610266575f91602483926040519485938492632e1a7d4d60e01b845260048401525af1801561027257610e48575b50610d48565b5f610e52916119fd565b8e610e42565b606460405162461bcd60e51b815260206004820152602060248201527f536c697070616765436865636b3a20496e73756666696369656e7420574554486044820152fd5b81518201602083820312610266576020610eba910160208401611b72565b610ec58d3331611978565b90518110610f16576001600160a01b038f16803b15610266575f90600460405180948193630d0e30db60e41b83525af1801561027257610f06575b50610d3c565b5f610f10916119fd565b8e610f00565b60405162461bcd60e51b815260206004820152601f60248201527f536c697070616765436865636b3a20496e73756666696369656e7420455448006044820152606490fd5b8151820160208382031261026657602083015167ffffffffffffffff811161026657830190610100828203126102665760405190610100820182811067ffffffffffffffff82111761096457604052610fb660208401611a1f565b8252610fc460408401611a1f565b916020810192835260608401519360408201948552610fe560808201611b1c565b926060830193845260a0820151918260020b8303610266576080840192835260c08101519081151582036102665760a0850191825261102660e08201611a1f565b9260c086019384526101008201519067ffffffffffffffff82116102665760200191016020019061105691611b2c565b938460e0820152516001600160a01b031695516001600160a01b03169651945162ffffff16925160020b9051151591516001600160a01b0316926040519661109d886119e0565b87526020870197885260408701958652606087019081526080870191825260a0870192835260c0870193845260e0870194855261010087019533875260405198899860208a0160209052516001600160a01b031660408a0152516001600160a01b031660608901525160808801525162ffffff1660a08701525160020b60c086015251151560e0850152516001600160a01b0316610100840152516101208301610120905261016083016111509161190d565b90516001600160a01b031661014083015203601f198101825261117390826119fd565b6040516348c8949160e01b81526020600482015290819061119890602483019061190d565b03817f00000000000000000000000000000000000000000000000000000000000000006001600160a01b031691815a5f948591f18015610272576111dd575b50610d30565b3d805f833e6111ec81836119fd565b810160208282031261026657815167ffffffffffffffff8111610266576112139201611b2c565b508e6111d7565b60c0828051810103126102665760405160c0810181811067ffffffffffffffff821117610964576040526020830151815261125760408401611a1f565b6020820190815261126a60608501611a1f565b906040830191825261127e60808601611a1f565b916060840192835260a08601519360ff60f81b8516948581036102665760808201526112ac60c08801611b1c565b60a0820190815290856115aa57600492939495506112e16001600160a01b038651166001600160a01b03885116835191611ac2565b60606001600160a01b0387511660405194858092630240bc6b60e21b82525afa928315610272575f905f94611545575b506dffffffffffffffffffffffffffff8091169316926001600160a01b038651166001600160a01b03865116115f1461153a5762ffffff9093915b519251169180156114e1578315928315806114d8575b1561148257606490046127100361271081116108be5761139091600a611389920490611bde565b9182611bde565b916103e884029384046103e81417156108be5782018092116108be57811561146e576001600160a01b0391829104935116915116115f1461145f576001600160a01b035f91925b51604051911660206113e981846119fd565b5f8352601f198101903690840137803b156102665761143a935f80946040519687958694859363022c0d9f60e01b85526004850152602484015233604484015260806064840152608483019061190d565b03925af180156102725761144f575b50610d24565b5f611459916119fd565b8e611449565b6001600160a01b035f926113d7565b634e487b7160e01b5f52601260045260245ffd5b60405162461bcd60e51b815260206004820152602860248201527f556e697377617056324c6962726172793a20494e53554646494349454e545f4c604482015267495155494449545960c01b6064820152608490fd5b50821515611362565b60405162461bcd60e51b815260206004820152602b60248201527f556e697377617056324c6962726172793a20494e53554646494349454e545f4960448201526a1394155517d05353d5539560aa1b6064820152608490fd5b9062ffffff9061134c565b9350506060833d82116115a2575b81611560606093836119fd565b810103126102665761157183611bc3565b604061157f60208601611bc3565b94015163ffffffff811603610266576dffffffffffffffffffffffffffff611311565b3d9150611553565b929491939291600160f81b036116b1575f6040946116526001600160a01b03808099511692511691828110928385146116915762ffffff8a6401000276ad995b511697519651169089519260208401528983015285606083015260808201526080815261161860a0826119fd565b875198899788968795630251596160e31b87523360048801526024870152604486015216606484015260a0608484015260a483019061190d565b03925af18015610272576116665750610d24565b604090813d811161168a575b61167c81836119fd565b81010312610266578e611449565b503d611672565b62ffffff8a73fffd8963efd1fc6a506488495d951d5263988d25996115ea565b60405162461bcd60e51b815260206004820152601460248201527f496e76616c696420706f6f6c2076617269616e740000000000000000000000006044820152606490fd5b50600160f91b8114610d1f565b60405162461bcd60e51b815260206004820152600f60248201527f496e76616c696420636f6d6d616e6400000000000000000000000000000000006044820152606490fd5b50600360f91b811115610d0c565b634e487b7160e01b5f52603260045260245ffd5b506001600160a01b0361177c82611931565b16611838575033315b818111156117f35760649161179991611978565b910135116117a357005b60405162461bcd60e51b815260206004820152602260248201527f536c697070616765436865636b3a20496e73756666696369656e74206f7574706044820152611d5d60f21b6064820152608490fd5b60405162461bcd60e51b815260206004820152601c60248201527f42616420537761703a204e6f20616d6f756e74207265636569766564000000006044820152606490fd5b61184b906118463391611931565b611a94565b611785565b610c3b6118603361184686611931565b94610c35565b62461bcd60e51b815260206004820152601560248201527f4f6e6c792063616c6c61626c652062792073656c6600000000000000000000006044820152606490fd5b9181601f840112156102665782359167ffffffffffffffff8311610266576020838186019501011161026657565b60606003198201126102665760043591602435916044359067ffffffffffffffff821161026657611909916004016118a8565b9091565b805180835260209291819084018484015e5f828201840152601f01601f1916010190565b356001600160a01b03811681036102665790565b903590601e1981360301821215610266570180359067ffffffffffffffff82116102665760200191813603831361026657565b919082039182116108be57565b35906001600160a01b038216820361026657565b359062ffffff8216820361026657565b9190826080910312610266576119be82611985565b916119cb60208201611985565b916119dd606060408401359301611999565b90565b610120810190811067ffffffffffffffff82111761096457604052565b90601f8019910116810190811067ffffffffffffffff82111761096457604052565b51906001600160a01b038216820361026657565b67ffffffffffffffff811161096457601f01601f191660200190565b929192611a5b82611a33565b91611a6960405193846119fd565b829481845281830111610266578281602093845f960137010152565b90816020910312610266575190565b602460106020939284936014526f70a082310000000000000000000000005f525afa601f3d11166020510290565b91906014526034526fa9059cbb0000000000000000000000005f5260205f6044601082855af1908160015f51141615611afe575b50505f603452565b3b153d171015611b0f575f80611af6565b6390b8ec185f526004601cfd5b519062ffffff8216820361026657565b81601f8201121561026657805190611b4382611a33565b92611b5160405194856119fd565b8284526020838301011161026657815f9260208093018386015e8301015290565b9190826020910312610266576040516020810181811067ffffffffffffffff8211176109645760405291518252565b9190916001600160a01b0383166001600160a01b038216105f146119dd579190565b51906dffffffffffffffffffffffffffff8216820361026657565b818102929181159184041417156108be5756fea2646970667358221220d8ec1785b7a8e413feeec76e5c163b98ab044e9cbd261f89d772ef24fd3a037864736f6c634300081e0033";

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   #[should_panic]
   async fn test_unauthorized_access_should_fail() {
      // ATTACK VECTOR 1: Steal ETH from the delegated EOA

      // Attacker: Bob
      // Victim: Alice

      // Attach the ZeusSwapDelegator contract to Alice
      // Bob will try to steal all the ETH from Alice by swapping ETH for USDT on a V4 Pool
      // Bob will do that by calling the public zSwap function on Alice address

      let chain_id = 1;
      let ctx = ZeusCtx::new();
      ctx.write(|ctx| ctx.providers.all_working());

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
         zeroForOne: eth_usdt.zero_for_one_v4(&currency_in),
         hooks: Address::ZERO,
         hookData: Bytes::default(),
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
         eprintln!("Call Reverted: {}", err);
         eprintln!("Output: {:?}", output);
         eprintln!("Gas Used: {}", res.gas_used());
         assert_eq!(err, "revert: Only callable by self");
         panic!("Bob's attack failed");
      }

      eprintln!("Gas Used: {}", res.gas_used());

      evm.cfg.disable_nonce_check = true;

      let alice_eth_balance = evm.balance(alice.address).unwrap().data;
      eprintln!("Alice's ETH Balance After: {}", alice_eth_balance);

      let bob_usdt_balance =
         simulate::erc20_balance(&mut evm, currency_out.address(), bob.address).unwrap();

      eprintln!("Bob's USDT Balance After: {}", bob_usdt_balance);

      assert_eq!(alice_eth_balance, U256::ZERO);
      assert!(bob_usdt_balance > U256::ZERO);
      eprintln!("Bob's attack successful");
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
      ctx.write(|ctx| ctx.providers.all_working());

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
      ctx.write(|ctx| ctx.providers.all_working());

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
