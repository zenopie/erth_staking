// src/msg.rs

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Binary, Uint128};

use crate::state::{Allocation, AllocationPercentage, State, UnbondingEntry, UserInfo};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct InstantiateMsg {
    pub erth_contract: Addr,
    pub erth_hash: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Withdraw {
        amount: Uint128,
    },
    Claim {},
    SetAllocation {
        percentages: Vec<AllocationPercentage>,
    },
    ClaimAllocation {
        allocation_id: u32,
    },
    AddAllocation {
        recieve_addr: Addr,
        recieve_hash: Option<String>,
        manager_addr: Option<Addr>,
        claimer_addr: Option<Addr>,
        use_send: bool,
    },
    EditAllocation {
        allocation_id: u32,
        key: String,
        value: Option<String>,
    },
    Receive {
        sender: Addr,
        from: Addr,
        amount: Uint128,
        memo: Option<String>,
        msg: Binary,
    },
    ClaimUnbonded {}, // Added new execute message
    DistributeAllocationRewards {}, // New message for allocation rewards distribution
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReceiveMsg {
    StakeErth {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SendMsg {
    AllocationSend {
        allocation_id: u32,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MigrateMsg {
    Migrate {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetState {},
    GetUserInfo {
        address: Addr,
    },
    GetAllocationOptions {},
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct StateResponse {
    pub state: State,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct AllocationOptionResponse {
    pub allocations: Vec<Allocation>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct UserInfoResponse {
    pub user_info: UserInfo,
    pub staking_rewards_due: Uint128,
    pub total_staked: Uint128,
    pub unbonding_entries: Vec<UnbondingEntry>, // Added unbonding entries to the response
}
