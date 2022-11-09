use near_sdk::{AccountId, ext_contract};

#[ext_contract(this_contract)]
trait Callbacks {
    fn query_staked_amount_callback(&mut self) -> u128;
}

#[ext_contract(ext_stake_contract)]
trait StakeContract {
    fn get_staked_amount(&self, _advisor_id: AccountId);
    fn update_apr(&self, _advisor_id: AccountId, _learner_vote: u8);
}