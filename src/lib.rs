/*
 * Example smart contract written in RUST
 *
 * Learn more about writing NEAR smart contracts with Rust:
 * https://near-docs.io/develop/Contract
 *
 */

use std::ops::{Mul, Sub};

use chrono::{DateTime, Utc};
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::json_types::U128;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{
    env, ext_contract, log, near_bindgen, require, AccountId, Balance, BorshStorageKey, Gas,
    PromiseError, Promise, PromiseOrValue, PromiseResult
};

pub mod external;
pub use crate::external::*;

pub const TGAS: u64 = 1_000_000_000_000;
pub const FT_TRANSFER_GAS: Gas = Gas(10_000_000_000_000);
pub const WITHDRAW_CALLBACK_GAS: Gas = Gas(10_000_000_000_000);
pub const FAUCET_CALLBACK_GAS: Gas = Gas(10_000_000_000_000);

#[ext_contract(ext_ft_contract)]
pub trait FungibleTokenCore {
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>);
    fn ft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        amount: U128,
        memo: Option<String>,
        msg: String,
    );
    fn ft_resolve_transfer(&mut self, sender_id: AccountId, receiver_id: AccountId, amount: U128);
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct RoomCreatedLog {
    advisor: AccountId,
    learner: AccountId,
    room_id: u128,
    start_time: DateTime<Utc>,
    amount_per_minute: Balance,
    minutes_last: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct RoomExtendedLog {
    advisor: AccountId,
    learner: AccountId,
    room_id: u128,
    amount_per_minute: Balance,
    minutes_last: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct ClaimedTokenLog {
    amount: Balance,
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PartialEq)]
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

// Define the contract structure
#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize)]
pub struct Contract {
    pub token_address: AccountId,
    pub verified_amount: Balance,
    pub room_list: LookupMap<u128, Room>,
}

#[derive(BorshDeserialize, BorshSerialize, BorshStorageKey)]
pub enum StorageKey {
    RoomIDKey,
}

// Implement the contract structure
#[near_bindgen]
impl Contract {
    #[init]
    pub fn new(_verified_amount: Balance, _token_address: AccountId) -> Self {
        Contract {
            token_address: _token_address,
            verified_amount: _verified_amount,
            room_list: LookupMap::new(StorageKey::RoomIDKey),
        }
    }

    // // Public method - returns the greeting saved, defaulting to DEFAULT_MESSAGE
    pub fn create_room(
        mut self,
        _advisor: AccountId,
        _amount_per_minute: u128,
        _room_id: u128,
        _minutes_lasts: i64,
    ) {
        let _learner = env::signer_account_id();
        
        let _staked_amount = Self::query_staked_amount(&self, _advisor.clone());
        env::promise_then(_staked_amount);
        // require!(_staked_amount >= self.verified_amount, "App: Not an advisor!");
        

        let _pending_amount = _amount_per_minute.mul(_minutes_lasts as u128);

        let room = Room {
            advisor: _advisor,
            learner: _learner,
            start_time: Utc::now().timestamp(),
            amount_per_minute: _amount_per_minute,
            minutes_last: _minutes_lasts,
            pending_amount: _pending_amount,
            claimed: false,
            reverted: false,
        };

        self.room_list.insert(&_room_id, &room);

        ext_ft_contract::ext(self.token_address.clone())
            .with_static_gas(FT_TRANSFER_GAS)
            .ft_transfer_call(
                env::current_account_id(),
                U128::from(_pending_amount),
                None,
                "spk_app".to_string(),
            )
            .then(
                ext_ft_contract::ext(self.token_address.clone()).ft_resolve_transfer(
                    room.learner,
                    env::current_account_id(),
                    U128::from(_pending_amount),
                ),
            );
    }

    pub fn extend_meeting(&self, _amount_per_minute: u128, _room_id: u128, _minutes_lasts: i64) {
        require!(
            self.room_list.contains_key(&_room_id) == true,
            "App: Room not existed!"
        );
        let mut room = self.room_list.get(&_room_id).unwrap();
        room.minutes_last += _minutes_lasts;
        room.pending_amount += _amount_per_minute.mul(_minutes_lasts as u128);

        ext_ft_contract::ext(self.token_address.clone())
            .with_static_gas(FT_TRANSFER_GAS)
            .ft_transfer_call(
                env::current_account_id(),
                U128::from(_amount_per_minute.mul(_minutes_lasts as u128)),
                None,
                "spk_app".to_string(),
            )
            .then(
                ext_ft_contract::ext(self.token_address.clone()).ft_resolve_transfer(
                    room.learner,
                    env::current_account_id(),
                    U128::from(_amount_per_minute.mul(_minutes_lasts as u128)),
                ),
            );
    }

    pub fn end_room(&self, _room_id: u128, _learner_vote: u8) {
        // signature?
        require!(
            self.room_list.contains_key(&_room_id) == true,
            "App: Room not existed!"
        );
        let mut room = self.room_list.get(&_room_id).unwrap();
        require!(room.claimed == false, "App: Already claimed!");
        require!(room.reverted == false, "App: Already reverted!");
        // require!(room.learner == recoverAddress, "App: Signature's learner not match");

        let minutes_last = Utc::now().timestamp().sub(room.start_time);
        require!(
            minutes_last >= room.minutes_last,
            "App: Too early to reveive token!"
        );

        ext_ft_contract::ext(self.token_address.clone())
            .with_static_gas(FT_TRANSFER_GAS)
            .ft_transfer(room.advisor, U128::from(room.pending_amount), None);

        room.claimed = true;
    }

    pub fn revert_token(&self, _room_id: u128) {
        require!(
            self.room_list.contains_key(&_room_id) == true,
            "App: Room not existed!"
        );
        let mut room = self.room_list.get(&_room_id).unwrap();

        ext_ft_contract::ext(self.token_address.clone())
            .with_static_gas(FT_TRANSFER_GAS)
            .ft_transfer(
                room.learner,
                U128::from(room.amount_per_minute.mul(room.minutes_last as u128)),
                None,
            );

        room.reverted = true;
    }

    #[private]
    pub fn query_staked_amount(&self, _advisor_id: AccountId) -> Promise {
        // Create a promise to call HelloNEAR.get_greeting()
        let promise = ext_stake_contract::ext(self.token_address.clone())
            .with_static_gas(Gas(5 * TGAS))
            .get_staked_amount(_advisor_id);

        return promise.then(
            // Create a promise to callback query_greeting_callback
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
        amount
    }
}