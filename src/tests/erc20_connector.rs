use crate::parameters::{FunctionCallArgs, SubmitResult};
use crate::prelude::*;
use crate::test_utils;
use crate::test_utils::AuroraRunner;
use crate::types::{AccountId, RawAddress};
use borsh::{BorshDeserialize, BorshSerialize};
use ethabi::Token;
use near_vm_logic::VMOutcome;
use near_vm_runner::VMError;
use secp256k1::SecretKey;
use sha3::Digest;

const INITIAL_BALANCE: u64 = 1000;
const INITIAL_NONCE: u64 = 0;

fn get_origin() -> AccountId {
    "evm".to_string()
}

pub struct CallResult {
    outcome: Option<VMOutcome>,
    error: Option<VMError>,
}

impl CallResult {
    fn check_ok(&self) {
        assert!(self.error.is_none());
    }

    fn value(&self) -> Vec<u8> {
        self.outcome
            .as_ref()
            .unwrap()
            .return_data
            .clone()
            .as_value()
            .unwrap()
    }

    fn submit_result(&self) -> SubmitResult {
        SubmitResult::try_from_slice(self.value().as_slice()).unwrap()
    }
}

fn keccak256(input: &[u8]) -> Vec<u8> {
    sha3::Keccak256::digest(input).to_vec()
}

fn get_selector(str_selector: &str) -> Vec<u8> {
    keccak256(str_selector.as_bytes())[..4].to_vec()
}

fn build_input(str_selector: &str, inputs: &[Token]) -> Vec<u8> {
    let sel = get_selector(str_selector);
    let inputs = ethabi::encode(inputs);
    [sel.as_slice(), inputs.as_slice()].concat().to_vec()
}

struct EthereumAccount {
    key: SecretKey,
    address: RawAddress,
}

impl test_utils::AuroraRunner {
    pub fn new() -> Self {
        test_utils::deploy_evm()
    }

    pub fn make_call(
        &mut self,
        method_name: &str,
        caller_account_id: String,
        input: Vec<u8>,
    ) -> CallResult {
        let (outcome, error) = self.call(method_name, caller_account_id, input);
        CallResult { outcome, error }
    }

    pub fn evm_call(
        &mut self,
        contract: RawAddress,
        input: Vec<u8>,
        origin: AccountId,
    ) -> CallResult {
        self.make_call(
            "call",
            origin,
            (FunctionCallArgs { contract, input }).try_to_vec().unwrap(),
        )
    }

    pub fn deploy_erc20_token(&mut self, nep141: AccountId) -> RawAddress {
        let result = self.make_call(
            "deploy_erc20_token",
            get_origin(),
            nep141.try_to_vec().unwrap(),
        );

        result.check_ok();

        Vec::<u8>::try_from_slice(result.value().as_slice())
            .unwrap()
            .try_into()
            .unwrap()
    }

    fn create_account(&mut self) -> EthereumAccount {
        let mut rng = rand::thread_rng();
        let source_account = SecretKey::random(&mut rng);
        let source_address = test_utils::address_from_secret_key(&source_account);
        self.create_address(source_address, INITIAL_BALANCE.into(), INITIAL_NONCE.into());
        EthereumAccount {
            key: source_account,
            address: source_address.into(),
        }
    }

    pub fn balance_of(&mut self, token: RawAddress, target: RawAddress, origin: AccountId) -> U256 {
        let input = build_input("balanceOf(address)", &[Token::Address(target.into())]);
        let result = self.evm_call(token, input, origin);
        result.check_ok();
        U256::from_big_endian(result.submit_result().result.as_slice())
    }

    pub fn mint(
        &mut self,
        token: RawAddress,
        target: RawAddress,
        amount: u64,
        origin: AccountId,
    ) -> CallResult {
        let input = build_input(
            "mint(address,uint256)",
            &[
                Token::Address(target.into()),
                Token::Uint(U256::from(amount).into()),
            ],
        );
        let result = self.evm_call(token, input, origin);
        println!("{:?}", result.error);
        result.check_ok();
        result
    }

    fn submit(&mut self) {}
}

#[test]
fn test_deploy_erc20_token() {
    let mut runner = AuroraRunner::new();
    runner.deploy_erc20_token("tt.testnet".to_string());
}

#[test]
fn test_mint() {
    let mut runner = AuroraRunner::new();
    let token = runner.deploy_erc20_token("tt.testnet".to_string());
    let address = runner.create_account();
    let balance = runner.balance_of(token, address.address, get_origin());
    assert_eq!(balance, U256::from(0));
    let amount = 10;
    runner.mint(token, address.address, amount, get_origin());
    let balance = runner.balance_of(token, address.address, get_origin());
    assert_eq!(balance, U256::from(balance));
}

#[test]
fn test_mint_not_admin() {
    let mut runner = AuroraRunner::new();
    let token = runner.deploy_erc20_token("tt.testnet".to_string());
    let address = runner.create_account();
    let balance = runner.balance_of(token, address.address, get_origin());
    assert_eq!(balance, U256::from(0));
    let amount = 10;
    runner.mint(token, address.address, amount, "not_admin".to_string());
    let balance = runner.balance_of(token, address.address, get_origin());
    assert_eq!(balance, U256::from(0));
}
