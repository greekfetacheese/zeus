#[cfg(test)]
mod tests {
   use crate::core::{BaseFee, ZeusCtx};
   use crate::gui::ui::dapps::uniswap::swap::get_relevant_pools;
   use crate::utils::{
      swap_quoter::{get_quote, get_quote_with_split_routing},
      zeus_router::encode_swap,
   };
   use std::str::FromStr;

   use zeus_eth::{
      abi::zeus::ZeusRouter,
      alloy_primitives::{Address, Bytes, TxKind, U256},
      alloy_provider::Provider,
      alloy_rpc_types::BlockId,
      amm::uniswap::{AnyUniswapPool, UniswapPool, UniswapV2Pool, UniswapV4Pool},
      currency::{Currency, ERC20Token, NativeCurrency},
      revm_utils::*,
      utils::{NumericValue, SecureSigner, address_book},
   };

   use alloy_sol_types::{SolCall, SolValue};

   const ROUTER_CODE: &str = "0x6101206040523461018957604051601f61246f38819003918201601f19168301916001600160401b038311848410176101755780849260a094604052833981010312610189576040519060a082016001600160401b038111838210176101755760405261006b8161018d565b80835261007a6020830161018d565b6020840190815261008d6040840161018d565b916040850192835260806100b5816100a76060880161018d565b96606089019788520161018d565b95019485526001600160a01b039081166080529051811660a0529051811660c0529051811660e052905116610100526040516122cd90816101a282396080518181816103bd0152818161105b0152818161111b01526111ed015260a051818181610177015281816108d401528181610bb901528181610cb5015281816115e60152611c1c015260c05181818161037a0152818161040f01526114a9015260e05181818161010d0152610337015261010051818181610bfc0152610c4b0152f35b634e487b7160e01b5f52604160045260245ffd5b5f80fd5b51906001600160a01b03821682036101895756fe608080604052600436101561001c575b50361561001a575f80fd5b005b5f3560e01c908162afc4b714610e315750806323a69e7514610c20578063695033f314610bdd5780636afdd85014610b9a57806391dd7346146103e1578063ad5c46481461039e578063e34e72831461035b578063f73e5aab146103185763fa461e331461008a575f61000f565b346102015761010160206001600160a01b03806100b46100a936611ebd565b819492940190611f5d565b604051630b4c774160e11b8152959098166001600160a01b03818116600488015299909416988916602486015262ffffff1660448501529298919790969295949291899081906064820190565b03816001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000165afa9788156101f6575f986102dc575b506001600160a01b03881680331490816102d2575b50156102755786101561026d5750925b83036102115715610205576001600160a01b037f00000000000000000000000000000000000000000000000000000000000000001690813b1561020157604051631b63c28b60e11b81526001600160a01b03918216600482015294811660248601529182166044850152911660648301525f908290608490829084905af180156101f6576101ec57005b5f61001a91612003565b6040513d5f823e3d90fd5b5f80fd5b50909161001a92612154565b60405162461bcd60e51b815260206004820152602e60248201527f556e697377617056335377617043616c6c6261636b3a20616d6f756e74546f5060448201526d30bc90109e9030b6b7bab73a24b760911b6064820152608490fd5b905092610162565b60405162461bcd60e51b815260206004820152602f60248201527f556e697377617056335377617043616c6c6261636b3a204d73672e73656e646560448201526e1c881a5cc81b9bdd0818481c1bdbdb608a1b6064820152608490fd5b905015155f610152565b9097506020813d602011610310575b816102f860209383612003565b810103126102015761030990612025565b965f61013d565b3d91506102eb565b34610201575f3660031901126102015760206040516001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000168152f35b34610201575f3660031901126102015760206040516001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000168152f35b34610201575f3660031901126102015760206040516001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000168152f35b60203660031901126102015760043567ffffffffffffffff81116102015761040d903690600401611e8f565b7f00000000000000000000000000000000000000000000000000000000000000006001600160a01b03811691823303610b2f578301926020818503126102015780359067ffffffffffffffff8211610201570192604084820312610201576040519361047885611fae565b61048181611f2c565b855260208101359067ffffffffffffffff82116102015701916101408383031261020157604051926104b284611fe6565b6104bb81611f2c565b84526104c960208201611f2c565b60208501908152604082013560408601526104e660608301611f40565b606086019081526080830135948560020b8603610201576080870195865261051060a08501611f50565b60a088015261052160c08501611f2c565b9360c0880194855260e081013567ffffffffffffffff81116102015781019382601f860112156102015761058f6101206105aa936105716001600160a01b039689602062ffffff9b359101612055565b60e08d01526105836101008201611f2c565b6101008d015201611f50565b6101208a0152828060208d019a808c52511691511690612202565b9390925116955160020b935116926040519160a083019083821067ffffffffffffffff831117610b1b576001600160a01b03916040521682526001600160a01b036020830193168352604082019586526060820190815260808201938452865160a08101511515805f14610afe5760406401000276ad925b015190600160ff1b8214610a58578a966040519161063f83611fca565b825260208201925f03835260408201936001600160a01b031684528a5160e00151946040519a8b988998633cf3645360e21b8a52516001600160a01b031660048a0152516001600160a01b031660248901525162ffffff1660448801525160020b6064870152516001600160a01b0316608486015251151560a48501525160c4840152516001600160a01b031660e48301526101048201610120905261012482016106e991611ef4565b03815a6020945f91f19182156101f6575f92610aca575b506001600160a01b0383515116843b156102015760405190632961046560e21b825260048201525f8160248183895af180156101f657610aba575b50825160a0015115610ab1578160801d5b600f0b5f811215610a6c576f7fffffffffffffffffffffffffffffff198114610a58576fffffffffffffffffffffffffffffffff905f0316916040845101518303610a1357835160a0015115610a0a57600f0b945b5f86600f0b13156109c557835180516001600160a01b031690816108ba5750505050602060049160405192838092630476982d60e21b8252875af180156101f65761088b575b505b51906001600160a01b03610100816020850151169301511690803b15610201575f92836064926fffffffffffffffffffffffffffffffff6040519788968795630b0d9c0960e01b8752600487015260248601521660448401525af180156101f65761087b575b610877604051610860602082612003565b5f8152604051918291602083526020830190611ef4565b0390f35b5f61088591612003565b8061084f565b6108ac9060203d6020116108b3575b6108a48183612003565b81019061208b565b50836107e7565b503d61089a565b610120015192939192156109b457506001600160a01b03807f000000000000000000000000000000000000000000000000000000000000000016925116916001600160a01b038551511693813b1561020157604051631b63c28b60e11b81526001600160a01b03948516600482015290841660248201529183166044830152929091166064820152905f908290608490829084905af180156101f6576109a4575b505b604051630476982d60e21b81526020816004815f875af180156101f657610985575b506107e9565b61099d9060203d6020116108b3576108a48183612003565b508361097f565b5f6109ae91612003565b8361095b565b6109c093919250612154565b61095d565b60405162461bcd60e51b815260206004820152601960248201527f56343a204e65676174697665206f75747075742064656c7461000000000000006044820152606490fd5b60801d946107a1565b60405162461bcd60e51b815260206004820152601b60248201527f56343a20616d6f756e74546f50617920213d20616d6f756e74496e00000000006044820152606490fd5b634e487b7160e01b5f52601160045260245ffd5b60405162461bcd60e51b815260206004820152601860248201527f56343a20506f73697469766520696e7075742064656c746100000000000000006044820152606490fd5b81600f0b61074c565b5f610ac491612003565b8561073b565b9091506020813d602011610af6575b81610ae660209383612003565b8101031261020157519085610700565b3d9150610ad9565b604073fffd8963efd1fc6a506488495d951d5263988d2592610622565b634e487b7160e01b5f52604160045260245ffd5b60405162461bcd60e51b815260206004820152603460248201527f556e697377617056345377617043616c6c6261636b3a204d73672e73656e646560448201527f72206973206e6f7420506f6f6c4d616e616765720000000000000000000000006064820152608490fd5b34610201575f3660031901126102015760206040516001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000168152f35b34610201575f3660031901126102015760206040516001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000168152f35b3461020157610c3f60206001600160a01b03806100b46100a936611ebd565b03816001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000165afa9788156101f6575f98610df5575b506001600160a01b0388168033149081610deb575b5015610d8e57861015610d865750925b8303610d2a5715610205576001600160a01b037f00000000000000000000000000000000000000000000000000000000000000001690813b1561020157604051631b63c28b60e11b81526001600160a01b03918216600482015294811660248601529182166044850152911660648301525f908290608490829084905af180156101f6576101ec57005b60405162461bcd60e51b815260206004820152602e60248201527f50616e63616b6556335377617043616c6c6261636b3a20616d6f756e74546f5060448201526d30bc90109e9030b6b7bab73a24b760911b6064820152608490fd5b905092610ca0565b60405162461bcd60e51b815260206004820152602f60248201527f50616e63616b6556335377617043616c6c6261636b3a204d73672e73656e646560448201526e1c881a5cc81b9bdd0818481c1bdbdb608a1b6064820152608490fd5b9050151589610c90565b9097506020813d602011610e29575b81610e1160209383612003565b8101031261020157610e2290612025565b9688610c7b565b3d9150610e04565b60203660031901126102015760043567ffffffffffffffff81116102015780600401908036039260a06003198501126102015742608483013510611e4d575060448101916001600160a01b03610e8684611f18565b16611e3b573331905b5f602484013595602219018612159584019260048401359267ffffffffffffffff84119160248601968560051b36038813945b610ecc84806120c8565b9050811015611d5457610edf84806120c8565b8291921015611d4057908101357fff0000000000000000000000000000000000000000000000000000000000000016908b610201578561020157866102015787811015611d4057610f43610f3c6024600584901b8c01018c6120c8565b3691612055565b91600760f81b8111611cfb578015611b06575b600160f81b81148015611af9575b611519575b600360f81b8114611280575b600160fa1b81146111b6575b600560f81b81146110fb575b600360f91b811461103b575b600760f81b14610fae575b6001915001610ec2565b60408280518101031261020157600191604051610fca81611fae565b6001600160a01b03610fed6040610fe360208601612025565b9485855201612025565b60208301908152921680611017575050611012906001600160a01b034791511661223f565b610fa4565b916001600160a01b038061102f61101295309061209a565b93511691511690612154565b825183016020848203126102015760206110599101602085016121cb565b7f00000000000000000000000000000000000000000000000000000000000000006001600160a01b0361108c308361209a565b911690813b15610201575f91602483926040519485938492632e1a7d4d60e01b845260048401525af180156101f6576110eb575b50516001600160a01b0316473082036110db575b5050610f99565b6110e49161223f565b8d806110d4565b5f6110f591612003565b8e6110c0565b825183016020848203126102015760206111199101602085016121cb565b7f000000000000000000000000000000000000000000000000000000000000000090476001600160a01b038316803b15610201575f90600460405180948193630d0e30db60e41b83525af180156101f6576111a6575b506001600160a01b03611182308461209a565b915116308103611195575b505050610f8d565b61119e92612154565b8d808061118d565b5f6111b091612003565b8f61116f565b604083805181010312610201576040516111cf81611fae565b6111db60208501612025565b815260408401519060208101918083527f0000000000000000000000000000000000000000000000000000000000000000916001600160a01b038316803b15610201575f90600460405180958193630d0e30db60e41b83525af19182156101f6576001600160a01b0392611270575b50511630810361125d575b505050610f81565b611268925191612154565b8d8080611255565b5f61127a91612003565b5f61124a565b8251830160208482031261020157602084015167ffffffffffffffff8111610201578401906101408282031261020157604051916112bd83611fe6565b6112c960208201612025565b83526112d760408201612025565b6020840152606081015160408401526112f2608082016121ae565b606084015260a08101518060020b810361020157608084015261131760c082016121be565b60a084015261132860e08201612025565b60c08401526101008101519067ffffffffffffffff82116102015761149c9361138a61014061147a9361136c6001600160a01b03966020805f9a019184010161210e565b60e085015261137e6101208201612025565b610100850152016121be565b6101208201526040519061139d82611fae565b338252602082019081526040519384926020808501525116604083015251604060608301526001600160a01b0381511660808301526001600160a01b0360208201511660a0830152604081015160c083015262ffffff60608201511660e0830152608081015160020b61010083015260a081015115156101208301526001600160a01b0360c08201511661014083015261012061144c60e08301516101406101608601526101c0850190611ef4565b6101008301516001600160a01b031661018085015291015115156101a083015203601f198101835282612003565b604051809381926348c8949160e01b8352602060048401526024830190611ef4565b0381836001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000165af180156101f6576114dc575b50610f75565b3d805f833e6114eb8183612003565b810160208282031261020157815167ffffffffffffffff811161020157611512920161210e565b508d6114d6565b6101008380518101031261020157604051610100810181811067ffffffffffffffff821117610b1b576040526020840151815261155860408501612025565b6020820190815261156b60608601612025565b916040810192835261157f60808701612025565b926060820193845260a08701519182608082015261159f60c08901612025565b9260a082019384526115b360e08a016121ae565b9060c083019182526115c86101008b016121be565b60e0840190815290806119885750511561195f576001600160a01b037f000000000000000000000000000000000000000000000000000000000000000016916001600160a01b038751166001600160a01b03825116936001600160a01b0388511690803b1561020157604051631b63c28b60e11b81523360048201526001600160a01b0393841660248201529583166044870152911660648501525f908490608490829084905af19283156101f65760049361194f575b505b60606001600160a01b0388511660405194858092630240bc6b60e21b82525afa9283156101f6575f905f946118ea575b506dffffffffffffffffffffffffffff8091169316926001600160a01b038751166001600160a01b03865116115f146118df5762ffffff9093915b519251169180156118865783159283158061187d575b156118275760649004612710036127108111610a585761173091600a611729920490612284565b9182612284565b916103e884029384046103e8141715610a58578201809211610a58578115611813576001600160a01b0391829104945116915116115f14611803576001600160a01b03805f93945b5116915116906020936040519461178f8187612003565b5f8652601f198101903690870137813b15610201575f80946117de6040519788968795869463022c0d9f60e01b8652600486015260248501526044840152608060648401526084830190611ef4565b03925af180156101f6576117f3575b50610f69565b5f6117fd91612003565b8d6117ed565b6001600160a01b03805f94611778565b634e487b7160e01b5f52601260045260245ffd5b60405162461bcd60e51b815260206004820152602860248201527f556e697377617056324c6962726172793a20494e53554646494349454e545f4c604482015267495155494449545960c01b6064820152608490fd5b50821515611702565b60405162461bcd60e51b815260206004820152602b60248201527f556e697377617056324c6962726172793a20494e53554646494349454e545f4960448201526a1394155517d05353d5539560aa1b6064820152608490fd5b9062ffffff906116ec565b9350506060833d8211611947575b8161190560609383612003565b810103126102015761191683612224565b604061192460208601612224565b94015163ffffffff811603610201576dffffffffffffffffffffffffffff6116b1565b3d91506118f8565b5f61195991612003565b5f61167f565b6004916119836001600160a01b038751166001600160a01b03895116835191612154565b611681565b91949390929091600103611ab457611a4d5f926001600160a01b0380604099511691511680821095868614611a8c5762ffffff6001600160a01b03806401000276ad9d5b5116995116955199511690511515918a519360208501528a84015288606084015233608084015260a083015260c082015260c08152611a0c60e082612003565b6001600160a01b038851998a9889978896630251596160e31b885260048801526024870152604486015216606484015260a0608484015260a4830190611ef4565b03925af180156101f657611a615750610f69565b604090813d8111611a85575b611a778183612003565b81010312610201578d6117ed565b503d611a6d565b62ffffff6001600160a01b038073fffd8963efd1fc6a506488495d951d5263988d259d6119cc565b60405162461bcd60e51b815260206004820152601460248201527f496e76616c696420706f6f6c2076617269616e740000000000000000000000006044820152606490fd5b50600160f91b8114610f64565b8251830160208482031261020157602084015167ffffffffffffffff8111610201578401808203919060e083126102015760405192611b4484611fae565b60c0811261020157608060405191611b5b83611fca565b12610201576040516080810181811067ffffffffffffffff821117610b1b57604052611b8960208401612025565b815260408301516001600160a01b0381168103610201576020820152611bb1606084016120fb565b6040820152611bc2608084016120fb565b60608201528152611bd560a08301612025565b602082015260c08201516040820152835260e08101519167ffffffffffffffff831161020157611c0c92602080920192010161210e565b8060208301526001600160a01b037f000000000000000000000000000000000000000000000000000000000000000016915190823b1561020157611cd69260405f80948251968795869485936302b67b5760e41b855233600486015265ffffffffffff606082516001600160a01b0381511660248901526001600160a01b036020820151166044890152828582015116606489015201511660848601526001600160a01b0360208201511660a4860152015160c484015261010060e4840152610104830190611ef4565b03925af180156101f657611ceb575b50610f56565b5f611cf591612003565b8d611ce5565b60405162461bcd60e51b815260206004820152600f60248201527f496e76616c696420636f6d6d616e6400000000000000000000000000000000006044820152606490fd5b634e487b7160e01b5f52603260045260245ffd5b50886001600160a01b03611d6782611f18565b16611e2257503331915b80831115611ddd578203918211610a58576064013511611d8d57005b60405162461bcd60e51b815260206004820152602260248201527f536c697070616765436865636b3a20496e73756666696369656e74206f7574706044820152611d5d60f21b6064820152608490fd5b60405162461bcd60e51b815260206004820152601c60248201527f42616420537761703a204e6f20616d6f756e74207265636569766564000000006044820152606490fd5b611e2e611e3591611f18565b339061209a565b91611d71565b611e47611e2e84611f18565b90610e8f565b62461bcd60e51b815260206004820152601160248201527f446561646c696e653a20457870697265640000000000000000000000000000006044820152606490fd5b9181601f840112156102015782359167ffffffffffffffff8311610201576020838186019501011161020157565b60606003198201126102015760043591602435916044359067ffffffffffffffff821161020157611ef091600401611e8f565b9091565b805180835260209291819084018484015e5f828201840152601f01601f1916010190565b356001600160a01b03811681036102015790565b35906001600160a01b038216820361020157565b359062ffffff8216820361020157565b3590811515820361020157565b91908260c091031261020157611f7282611f2c565b91611f7f60208201611f2c565b91604082013591611f9260608201611f2c565b91611fab60a0611fa460808501611f40565b9301611f50565b90565b6040810190811067ffffffffffffffff821117610b1b57604052565b6060810190811067ffffffffffffffff821117610b1b57604052565b610140810190811067ffffffffffffffff821117610b1b57604052565b90601f8019910116810190811067ffffffffffffffff821117610b1b57604052565b51906001600160a01b038216820361020157565b67ffffffffffffffff8111610b1b57601f01601f191660200190565b92919261206182612039565b9161206f6040519384612003565b829481845281830111610201578281602093845f960137010152565b90816020910312610201575190565b602460106020939284936014526f70a082310000000000000000000000005f525afa601f3d11166020510290565b903590601e1981360301821215610201570180359067ffffffffffffffff82116102015760200191813603831361020157565b519065ffffffffffff8216820361020157565b81601f820112156102015780519061212582612039565b926121336040519485612003565b8284526020838301011161020157815f9260208093018386015e8301015290565b91906014526034526fa9059cbb0000000000000000000000005f5260205f6044601082855af1908160015f51141615612190575b50505f603452565b3b153d1710156121a1575f80612188565b6390b8ec185f526004601cfd5b519062ffffff8216820361020157565b5190811515820361020157565b9190826020910312610201576040516020810181811067ffffffffffffffff821117610b1b576040526121fe8193612025565b9052565b9190916001600160a01b0383166001600160a01b038216105f14611fab579190565b51906dffffffffffffffffffffffffffff8216820361020157565b814710612277575f3881808585620186a0f11561225a575050565b601691600b915f526073825360ff602053f01561227357565b3838fd5b63b12d13eb5f526004601cfd5b81810292918115918404141715610a585756fea2646970667358221220060dca4795a58b5aa4c324c89e27e444578663af19676a6a0596cc689dce9e6164736f6c634300081e0033";

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn v2_swap_erc20_to_erc20() {
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

      test_swap(
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
   async fn test_eth_to_erc20_v2_swap() {
      let chain_id = 1;

      let pool: AnyUniswapPool = UniswapV2Pool::weth_uni().into();
      let currency_in = Currency::from(NativeCurrency::from(chain_id));
      let currency_out = pool.quote_currency().clone();
      let amount_in = NumericValue::parse_to_wei("10", currency_in.decimals());

      let swap_on_v2 = true;
      let swap_on_v3 = true;
      let swap_on_v4 = true;
      let max_hops = 2;
      let max_routes = 1;
      let with_split_routing = true;

      test_swap(
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
   async fn v4_single_swap() {
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

      test_swap(
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
   async fn big_swap_from_usdt_to_eth() {
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

      test_swap(
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
   async fn big_swap_from_usdt_to_weth() {
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

      test_swap(
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
   async fn big_swap_from_link_to_eth_mainnet() {
      let chain_id = 1;

      let currency_in = Currency::from(ERC20Token::link());
      let currency_out = Currency::from(NativeCurrency::from(chain_id));
      let amount_in = NumericValue::parse_to_wei("10000", currency_in.decimals());

      let swap_on_v2 = true;
      let swap_on_v3 = true;
      let swap_on_v4 = true;
      let max_hops = 10;
      let max_routes = 10;
      let with_split_routing = true;

      test_swap(
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
         Vec::new(),
      )
      .await
      .unwrap();
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn big_swap_from_uni_to_eth_mainnet() {
      let chain_id = 1;

      let uni_pool: AnyUniswapPool = UniswapV2Pool::weth_uni().into();
      let currency_in = uni_pool.quote_currency().clone();
      let currency_out = Currency::from(NativeCurrency::from(chain_id));
      let amount_in = NumericValue::parse_to_wei("90000", currency_in.decimals());

      let swap_on_v2 = true;
      let swap_on_v3 = false;
      let swap_on_v4 = true;
      let max_hops = 5;
      let max_routes = 5;
      let with_split_routing = false;

      test_swap(
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
         Vec::new(),
      )
      .await
      .unwrap();
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn single_v4_swap_erc20_to_erc20_mainnet() {
      let chain_id = 1;

      let pool: AnyUniswapPool = UniswapV4Pool::usdc_usdt().into();
      let currency_in = Currency::from(ERC20Token::usdc());
      let currency_out = Currency::from(ERC20Token::usdt());
      let amount_in = NumericValue::parse_to_wei("10000", currency_in.decimals());

      let swap_on_v2 = true;
      let swap_on_v3 = true;
      let swap_on_v4 = true;
      let max_hops = 2;
      let max_routes = 1;
      let with_split_routing = true;

      test_swap(
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

   async fn test_swap(
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
         amount_in.abbreviated(),
         currency_in.symbol(),
         currency_out.symbol(),
         amount_out.abbreviated()
      );
      eprintln!("Swap Steps Length: {}", swap_steps.len());

      for swap in &swap_steps {
         eprintln!(
            "Swap Step: {} (Wei: {}) {} -> {} (Wei: {}) {} {} ({})",
            swap.amount_in.abbreviated(),
            swap.amount_in.wei(),
            swap.currency_in.symbol(),
            swap.amount_out.abbreviated(),
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
      let signer = SecureSigner::from(alice.key.clone());

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
      let router_addr = deploy_router(&mut evm, alice.address, chain)?;
      let deadline = 5;

      let swap_params = encode_swap(
         ctx.clone(),
         Some(router_addr),
         None,
         chain,
         swap_steps,
         amount_in.wei(),
         min_amount_out.wei(),
         currency_in.clone(),
         currency_out.clone(),
         signer.clone(),
         deadline,
      )
      .await?;

      let permit2 = address_book::permit2_contract(chain).unwrap();

      if swap_params.permit2_needs_approval() {
         simulate::approve_token(
            &mut evm,
            currency_in.address(),
            alice.address,
            permit2,
            U256::MAX,
         )
         .unwrap();
      }

      let router_balance = evm.balance(router_addr).unwrap().data;
      let balance = NumericValue::format_wei(router_balance, 18);
      eprintln!(
         "Router ETH Balance Before: {}",
         balance.abbreviated()
      );

      evm.tx.caller = alice.address;
      evm.tx.data = swap_params.call_data.clone();
      evm.tx.value = swap_params.value.clone();
      evm.tx.kind = TxKind::Call(router_addr);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      if !res.is_success() {
         let err = revert_msg(&output);
         eprintln!("Call Reverted: {}", err);
         eprintln!("Output: {:?}", output);
         eprintln!("Gas Used: {}", res.gas_used());
         panic!("Router Call Failed");
      }

      eprintln!("Gas Used: {}", res.gas_used());

      let router_balance = evm.balance(router_addr).unwrap().data;
      let balance = NumericValue::format_wei(router_balance, 18);
      eprintln!(
         "Router ETH Balance After: {}",
         balance.abbreviated()
      );

      let router_currency_out_balance = if currency_out.is_erc20() {
         let balance =
            simulate::erc20_balance(&mut evm, currency_out.address(), router_addr).unwrap();
         NumericValue::format_wei(balance, currency_out.decimals())
      } else {
         balance
      };

      eprintln!(
         "Router Currency Out Balance: {} {}",
         currency_out.symbol(),
         router_currency_out_balance.abbreviated()
      );

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
         amount_out.abbreviated()
      );

      eprintln!(
         "{} Got from Swap: {}",
         currency_out.symbol(),
         balance.abbreviated()
      );

      Ok(())
   }

   fn deploy_router(
      evm: &mut Evm2<ForkDB>,
      caller: Address,
      chain: u64,
   ) -> Result<Address, anyhow::Error> {
      let weth = address_book::weth(chain)?;
      let permit2 = address_book::permit2_contract(chain)?;
      let v4_pool_manager = address_book::uniswap_v4_pool_manager(chain)?;
      let uni_v3_factory = address_book::uniswap_v3_factory(chain)?;
      let pancake_v3_factory = address_book::pancakeswap_v3_factory(chain)?;

      let deploy_params = ZeusRouter::DeployParams {
         weth,
         permit2,
         v4PoolManager: v4_pool_manager,
         uniswapV3Factory: uni_v3_factory,
         pancakeSwapV3Factory: pancake_v3_factory,
      }
      .abi_encode();

      let code = Bytes::from_str(ROUTER_CODE)?;
      let bytecode = [code.as_ref(), &deploy_params].concat();

      evm.tx.caller = caller;
      evm.tx.data = bytecode.into();
      evm.tx.kind = TxKind::Create;
      evm.tx.value = U256::ZERO;

      let res = evm.transact_commit(evm.tx.clone())?;
      eprintln!("Gas Used for Deploy: {}", res.gas_used());

      let address = match res {
         ExecutionResult::Success { output, .. } => output.address().cloned(),
         _ => None,
      };

      Ok(address.unwrap())
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn test_deploy_router() {
      let ctx = ZeusCtx::new();

      let chain = 1;

      let client = ctx.get_client(chain).await.unwrap();
      let block = client.get_block(BlockId::latest()).await.unwrap().unwrap();
      let fork_factory = ForkFactory::new_sandbox_factory(client.clone(), chain, None, None);
      let fork_db = fork_factory.new_sandbox_fork();
      let mut evm = new_evm(chain.into(), Some(&block), fork_db);

      let weth = address_book::weth(chain).unwrap();
      let permit2 = address_book::permit2_contract(chain).unwrap();
      let v4_pool_manager = address_book::uniswap_v4_pool_manager(chain).unwrap();
      let uni_v3_factory = address_book::uniswap_v3_factory(chain).unwrap();
      let pancake_v3_factory = address_book::pancakeswap_v3_factory(chain).unwrap();

      let deploy_params = ZeusRouter::DeployParams {
         weth,
         permit2,
         v4PoolManager: v4_pool_manager,
         uniswapV3Factory: uni_v3_factory,
         pancakeSwapV3Factory: pancake_v3_factory,
      }
      .abi_encode();

      let code = Bytes::from_str(ROUTER_CODE).unwrap();
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

      evm.tx.data = ZeusRouter::WETHCall {}.abi_encode().into();
      evm.tx.kind = TxKind::Call(address);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      let weth_address = ZeusRouter::WETHCall::abi_decode_returns(output.as_ref()).unwrap();
      assert_eq!(weth_address, weth);

      evm.tx.data = ZeusRouter::PERMIT2Call {}.abi_encode().into();
      evm.tx.kind = TxKind::Call(address);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      let permit2_address = ZeusRouter::PERMIT2Call::abi_decode_returns(output.as_ref()).unwrap();
      assert_eq!(permit2_address, permit2);

      evm.tx.data = ZeusRouter::V4_POOL_MANAGERCall {}.abi_encode().into();
      evm.tx.kind = TxKind::Call(address);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      let v4_pool_manager_address =
         ZeusRouter::V4_POOL_MANAGERCall::abi_decode_returns(output.as_ref()).unwrap();
      assert_eq!(v4_pool_manager_address, v4_pool_manager);

      evm.tx.data = ZeusRouter::UNISWAP_V3_FACTORYCall {}.abi_encode().into();
      evm.tx.kind = TxKind::Call(address);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      let uni_v3_factory_address =
         ZeusRouter::UNISWAP_V3_FACTORYCall::abi_decode_returns(output.as_ref()).unwrap();
      assert_eq!(uni_v3_factory_address, uni_v3_factory);

      evm.tx.data = ZeusRouter::PANCAKE_SWAP_V3_FACTORYCall {}.abi_encode().into();
      evm.tx.kind = TxKind::Call(address);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      let pancake_v3_factory_address =
         ZeusRouter::PANCAKE_SWAP_V3_FACTORYCall::abi_decode_returns(output.as_ref()).unwrap();
      assert_eq!(pancake_v3_factory_address, pancake_v3_factory);
   }
}
