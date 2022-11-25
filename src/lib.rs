use ed25519_dalek::Verifier;
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::json_types::U128;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{
    assert_one_yocto, bs58, env, ext_contract, near_bindgen, require, AccountId, Balance,
    BorshStorageKey, Gas, PanicOnDefault, Promise, PromiseError, PromiseOrValue, ONE_YOCTO,
};
use std::ops::{Mul, Sub};
use std::time::SystemTime;

pub mod external;
pub use crate::external::*;

pub const TGAS: u64 = 1_000_000_000_000;
pub const FT_TRANSFER_GAS: Gas = Gas(10_000_000_000_000);
pub const WITHDRAW_CALLBACK_GAS: Gas = Gas(10_000_000_000_000);
pub const FAUCET_CALLBACK_GAS: Gas = Gas(10_000_000_000_000);

#[ext_contract(ext_ft_contract)]
pub trait FungibleTokenCore {
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>);
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct RoomCreatedLog {
    advisor: AccountId,
    learner: AccountId,
    room_id: u128,
    start_time: i64,
    amount_per_minute: Balance,
    minutes_last: i64,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct RoomExtendedLog {
    advisor: AccountId,
    learner: AccountId,
    room_id: u128,
    amount_per_minute: Balance,
    minutes_last: i64,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct ClaimedTokenLog {
    amount: Balance,
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PartialEq, Clone)]
pub struct Room {
    advisor: AccountId,
    learner: AccountId,
    start_time: i64,
    amount_per_minute: Balance,
    minutes_last: i64,
    pending_amount: u128,
    claimed: bool,
    reverted: bool,
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Contract {
    pub owner: AccountId,
    pub staking_address: AccountId,
    pub token_address: AccountId,
    pub verified_amount: Balance,
    pub room_list: LookupMap<u128, Room>,
}

#[derive(BorshDeserialize, BorshSerialize, BorshStorageKey)]
pub enum StorageKey {
    RoomIDKey,
}

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new(
        _verified_amount: U128,
        _token_address: AccountId,
        _staking_address: AccountId,
    ) -> Self {
        Contract {
            owner: env::signer_account_id(),
            staking_address: _staking_address,
            token_address: _token_address,
            verified_amount: u128::from(_verified_amount),
            room_list: LookupMap::new(StorageKey::RoomIDKey),
        }
    }

    // call ft_transfer_call on token contract to do create_room/extend_meeting fn called by token contract
    pub fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
        _advisor: Option<AccountId>,
        _amount_per_minute: Option<U128>,
        _room_id: Option<U128>,
        _minutes_lasts: Option<i64>,
        _signature: Option<Vec<u8>>,
        _signer: Option<Vec<u8>>,
    ) -> PromiseOrValue<U128> {
        let _amount_per_minute = u128::from(_amount_per_minute.unwrap());
        let _room_id = u128::from(_room_id.unwrap());
        // require!(
        //     Self::verify(
        //         &self,
        //         _signature.unwrap(),
        //         _signer.unwrap(),
        //         _advisor.clone().unwrap()
        //     ) == true,
        //     "There was an error verifying advisor's signature"
        // );

        if msg == "create_room" {
            Self::query_staked_amount(&self, _advisor.clone().unwrap());

            let _pending_amount = _amount_per_minute.mul(_minutes_lasts.unwrap() as u128);

            let room = Room {
                advisor: _advisor.unwrap(),
                learner: sender_id,
                start_time: Self::now(),
                amount_per_minute: _amount_per_minute,
                minutes_last: _minutes_lasts.unwrap(),
                pending_amount: _pending_amount,
                claimed: false,
                reverted: false,
            };

            self.room_list.insert(&_room_id, &room);
        } else if msg == "extend_room" {
            let mut room = self.room_list.get(&_room_id).unwrap();
            require!(
                self.room_list.contains_key(&_room_id) == true,
                "App: Room not existed!"
            );
            require!(sender_id == room.learner, "App: Invalid learner!");
            room.minutes_last += _minutes_lasts.unwrap();
            room.pending_amount += _amount_per_minute.mul(_minutes_lasts.unwrap() as u128);

            self.room_list.insert(&_room_id, &room);
        }

        return PromiseOrValue::Value(U128(0));
    }

    // advisor sign
    #[payable]
    pub fn end_room(
        &mut self,
        _room_id: U128,
        _learner_vote: u8,
        _signature: Vec<u8>,
        _signer: Vec<u8>,
    ) {
        assert_one_yocto();
        let _room_id = u128::from(_room_id);
        require!(
            self.room_list.contains_key(&_room_id) == true,
            "App: Room not existed!"
        );
        let mut room = self.room_list.get(&_room_id).unwrap();
        // require!(
        //     Self::verify(&self, _signature, _signer, room.advisor.clone()) == true,
        //     "There was an error verifying advisor's signature"
        // );

        require!(room.claimed == false, "App: Already claimed!");
        require!(room.reverted == false, "App: Already reverted!");

        // let minutes_last = Utc::now().timestamp().sub(room.start_time);
        // require!(
        //     minutes_last >= room.minutes_last,
        //     "App: Too early to reveive token!"
        // );

        ext_stake_contract::ext(self.token_address.clone())
            .with_static_gas(FT_TRANSFER_GAS)
            .update_apr(env::signer_account_id(), _learner_vote);

        ext_ft_contract::ext(self.token_address.clone())
            .with_static_gas(FT_TRANSFER_GAS)
            .ft_transfer(
                room.advisor.clone(),
                U128::from(room.pending_amount * 95 / 100),
                None,
            );

        room.claimed = true;
        self.room_list.insert(&_room_id, &room);
    }

    // advisor leave meeting at least 10 minutes then leaner can revert their tokens
    // fe check time advisor leave. If time > 10 minutes, fe will allow learner do this function and create a signature for this fn
    // admin sign
    #[payable]
    pub fn revert_token(&mut self, _room_id: U128, _signature: Vec<u8>, _signer: Vec<u8>) {
        assert_one_yocto();
        let _room_id = u128::from(_room_id);
        // require!(
        //     Self::verify(&self, _signature, _signer, self.owner.clone()) == true,
        //     "There was an error verifying admin's signature"
        // );

        require!(
            self.room_list.contains_key(&_room_id) == true,
            "App: Room not existed!"
        );
        let mut room = self.room_list.get(&_room_id).unwrap();

        require!(
            Self::now().sub(room.start_time) < room.minutes_last,
            "App: Room already ended!"
        );

        ext_ft_contract::ext(self.token_address.clone())
            .with_static_gas(FT_TRANSFER_GAS)
            .ft_transfer(
                room.learner.clone(),
                U128::from(room.amount_per_minute.mul(room.minutes_last as u128) * 95 / 100),
                None,
            );

        room.reverted = true;
        self.room_list.insert(&_room_id, &room);
    }

    #[private]
    pub fn query_staked_amount(&self, _advisor_id: AccountId) -> Promise {
        let promise = ext_stake_contract::ext(self.token_address.clone())
            .with_static_gas(Gas(5 * TGAS))
            .get_staked_amount(_advisor_id);

        return promise.then(
            // Create a promise to callback query_staked_amount
            Self::ext(env::current_account_id())
                .with_static_gas(Gas(5 * TGAS))
                .query_staked_amount_callback(),
        );
    }

    #[private]
    pub fn query_staked_amount_callback(
        &self,
        #[callback_result] call_result: Result<u128, PromiseError>,
    ) -> u128 {
        // Check if the promise succeeded by calling the method outlined in external.rs
        if call_result.is_err() {
            require!(1 != 1, "There was an error contacting staking contract");
        }

        // Return the amount
        let amount = call_result.unwrap();
        require!(amount >= self.verified_amount, "App: Not an advisor!");
        amount
    }

    #[private]
    pub fn verify(
        &self,
        _signature: Vec<u8>,
        _signer_public_key: Vec<u8>,
        _account_id: AccountId,
    ) -> bool {
        // https://stackoverflow.com/questions/70041130/how-to-verify-secp256k1-signed-message-in-smart-contract
        // verify signature of app creator
        let signature = ed25519_dalek::Signature::try_from(_signature.as_ref())
            .expect("Signature should be a valid array of 64 bytes [13, 254, 123, ...]");
        let public_key = ed25519_dalek::PublicKey::from_bytes(
            &bs58::decode(
                // public key "H5ANpdUoXVwhYBgAgEi1ieMQZKJbwxjPJtHX4vkVcSnF",
                _signer_public_key,
            )
            .into_vec()
            .unwrap(),
        )
        .unwrap();
        if let Ok(_) = public_key.verify(_account_id.as_bytes(), &signature) {
            return true;
        } else {
            return false;
        }
    }

    #[private]
    pub fn now() -> i64 {
        return SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
    }
}
