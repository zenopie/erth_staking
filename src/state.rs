// src/state.rs

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128, Timestamp};

use secret_toolkit_storage::{Keymap, Item};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct State {
    pub contract_manager: Addr,
    pub erth_token_contract: Addr,
    pub erth_token_hash: String,
    pub total_staked: Uint128,
    pub total_allocations: Uint128,
    pub allocation_counter: u32,
    pub last_upkeep: Timestamp,
}

pub static STATE: Item<State> = Item::new(b"state");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Allocation {
    pub allocation_id: u32,
    pub accumulated_rewards: Uint128,
    pub recieve_addr: Addr,
    pub recieve_hash: Option<String>,
    pub manager_addr: Option<Addr>,
    pub claimer_addr: Option<Addr>,
    pub use_send: bool,
    pub amount_allocated: Uint128,
    pub last_claim: Timestamp,

}

pub static ALLOCATION_OPTIONS: Item<Vec<Allocation>> = Item::new(b"allocation_options");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct UserInfo {
    pub staked_amount: Uint128,
    pub last_claim: Timestamp,
    pub allocations: Vec<UserAllocation>,
    pub percentages: Vec<AllocationPercentage>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct UserAllocation {
    pub allocation_id: u32,
    pub amount_allocated: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct AllocationPercentage {
    pub allocation_id: u32,
    pub percentage: Uint128,
}

pub static USER_INFO: Keymap<Addr, UserInfo> = Keymap::new(b"user_info");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct UnbondingEntry {
    pub amount: Uint128,
    pub unbonding_time: Timestamp,
}

pub static UNBONDING_INFO: Keymap<Addr, Vec<UnbondingEntry>> = Keymap::new(b"unbonding_info");
