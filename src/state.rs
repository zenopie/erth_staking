use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128};

use secret_toolkit_storage::{Keymap, Item};
use crate::msg::{AllocationPercentage};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct State {
    pub erth_contract: Addr,
    pub erth_hash: String,
    pub total_deposits: Uint128,
    pub total_allocations: Uint128,
}

pub static STATE: Item<State> = Item::new(b"state");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Allocation {
    pub address: Addr,
    pub amount: Uint128,
}

pub static ALLOCATION_OPTIONS: Item<Vec<Allocation>> = Item::new(b"allocation_options");

pub static DEPOSIT_AMOUNTS: Keymap<Addr, Uint128> = Keymap::new(b"deposit_amounts");

pub static INDIVIDUAL_ALLOCATIONS: Keymap<Addr, Vec<Allocation>> = Keymap::new(b"individual_allocations");

pub static INDIVIDUAL_PERCENTAGES: Keymap<Addr, Vec<AllocationPercentage>> = Keymap::new(b"individual_percentages");




