// src/contract.rs

use cosmwasm_std::{
    entry_point, from_binary, to_binary, Addr, Binary, CosmosMsg, Deps, DepsMut, Env, MessageInfo,
    Response, StdError, StdResult, Timestamp, Uint128, WasmMsg,
};
use secret_toolkit::snip20;

use crate::msg::{
    ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg, ReceiveMsg, SendMsg, StateResponse,
    UserInfoResponse, AllocationOptionResponse,
};
use crate::state::{
    Allocation, AllocationPercentage,
    ALLOCATION_OPTIONS, STATE, State, UnbondingEntry, USER_INFO, UNBONDING_INFO, UserAllocation,
    UserInfo,
};

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, StdError> {
    let state = State {
        contract_manager: info.sender,
        erth_token_contract: msg.erth_contract,
        erth_token_hash: msg.erth_hash,
        total_staked: Uint128::zero(),
        total_allocations: Uint128::zero(),
        allocation_counter: 0,
        last_upkeep: env.block.time,
    };
    STATE.save(deps.storage, &state)?;

    let allocation_options = Vec::new();
    ALLOCATION_OPTIONS.save(deps.storage, &allocation_options)?;

    // Register the contract as a receiver for the ERTH token
    let message = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: state.erth_token_contract.to_string(),
        code_hash: state.erth_token_hash.clone(),
        msg: to_binary(&snip20::HandleMsg::RegisterReceive {
            code_hash: env.contract.code_hash.clone(),
            padding: None,
        })?,
        funds: vec![],
    });
    Ok(Response::new()
        .add_message(message)
        .add_attribute("action", "instantiate"))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, StdError> {
    match msg {
        ExecuteMsg::Withdraw { amount } => execute_withdraw(deps, env, info, amount),
        ExecuteMsg::Claim {} => execute_claim_staking_rewards(deps, env, info),
        ExecuteMsg::SetAllocation { percentages } => {
            execute_set_allocation(deps, env, info, percentages)
        }
        ExecuteMsg::ClaimAllocation { allocation_id } => {
            execute_claim_allocation(deps, env, info, allocation_id)
        }
        ExecuteMsg::EditAllocation {
            allocation_id,
            key,
            value,
        } => execute_edit_allocation(deps, info, allocation_id, key, value),
        ExecuteMsg::AddAllocation {
            recieve_addr,
            recieve_hash,
            manager_addr,
            claimer_addr,
            use_send,
        } => execute_add_allocation(
            deps,
            env,
            info,
            recieve_addr,
            recieve_hash,
            manager_addr,
            claimer_addr,
            use_send,
        ),
        ExecuteMsg::Receive {
            sender,
            from,
            amount,
            msg,
            memo: _,
        } => try_receive(deps, env, info, sender, from, amount, msg),
        ExecuteMsg::ClaimUnbonded {} => execute_claim_unbonded(deps, env, info),
        ExecuteMsg::DistributeAllocationRewards {} => execute_distribute_allocation_rewards(deps, env, info),
    }
}

fn execute_edit_allocation(
    deps: DepsMut,
    info: MessageInfo,
    allocation_id: u32,
    key: String,
    value: Option<String>,
) -> StdResult<Response> {
    // Load the current state
    let state = STATE.load(deps.storage)?;

    // Load allocation options from storage
    let mut allocations = ALLOCATION_OPTIONS.load(deps.storage)?;

    // Find the allocation by ID
    let allocation = allocations
        .iter_mut()
        .find(|alloc| alloc.allocation_id == allocation_id)
        .ok_or_else(|| StdError::generic_err("Allocation not found"))?;

    // Check if the sender is authorized to edit the allocation
    if info.sender != state.contract_manager {
        if let Some(manager_addr) = &allocation.manager_addr {
            if &info.sender != manager_addr {
                return Err(StdError::generic_err(
                    "Unauthorized: Only the allocation manager or contract manager can edit this allocation",
                ));
            }
        } else {
            return Err(StdError::generic_err(
                "Unauthorized: Only the contract manager can edit this allocation",
            ));
        }
    }

    // Match the key to determine which field to update
    match key.as_str() {
        "recieve_addr" => {
            if let Some(value) = value {
                let new_addr = deps.api.addr_validate(&value)?;
                allocation.recieve_addr = new_addr;
            } else {
                return Err(StdError::generic_err("recieve_addr cannot be None"));
            }
        }
        "recieve_hash" => {
            allocation.recieve_hash = value;
        }
        "manager_addr" => {
            allocation.manager_addr = if let Some(value) = value {
                Some(deps.api.addr_validate(&value)?)
            } else {
                None
            };
        }
        "claimer_addr" => {
            allocation.claimer_addr = if let Some(value) = value {
                Some(deps.api.addr_validate(&value)?)
            } else {
                None
            };
        }
        "use_send" => {
            if let Some(value) = value {
                let new_use_send = value
                    .parse::<bool>()
                    .map_err(|_| StdError::generic_err("Invalid value for use_send"))?;
                allocation.use_send = new_use_send;
            } else {
                return Err(StdError::generic_err("use_send cannot be None"));
            }
        }
        _ => {
            return Err(StdError::generic_err(
                "Invalid key for allocation update",
            ));
        }
    }

    // Save the updated allocation options back to storage
    ALLOCATION_OPTIONS.save(deps.storage, &allocations)?;

    Ok(Response::new()
        .add_attribute("action", "edit_allocation")
        .add_attribute("allocation_id", allocation_id.to_string())
        .add_attribute("updated_field", key))
}

pub fn execute_claim_allocation(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    allocation_id: u32,
) -> StdResult<Response> {
    // Load the current state
    let state = STATE.load(deps.storage)?;

    // Find the allocation by ID
    let mut allocation_options = ALLOCATION_OPTIONS.load(deps.storage)?;
    let allocation = allocation_options
        .iter_mut()
        .find(|alloc| alloc.allocation_id == allocation_id)
        .ok_or_else(|| StdError::generic_err("Allocation not found"))?;

    // If there's a claimer address, check that the info.sender is the claimer
    if let Some(claimer_addr) = &allocation.claimer_addr {
        if &info.sender != claimer_addr {
            return Err(StdError::generic_err(
                "Unauthorized: Only the claimer can claim this allocation",
            ));
        }
    }

    // Get the accumulated rewards
    let rewards_to_claim = allocation.accumulated_rewards;
    
    // If no rewards available, return a success response instead of an error
    if rewards_to_claim.is_zero() {
        return Ok(Response::new()
            .add_attribute("action", "claim_allocation")
            .add_attribute("allocation_id", allocation_id.to_string())
            .add_attribute("rewards_claimed", "0")
            .add_attribute("message", "No rewards available to claim"));
    }

    let mut messages = Vec::new();

    // Prepare the minting message based on the `use_send` flag
    if allocation.use_send {
        // Mint to the staking contract and trigger the receive function
        let mint_msg = snip20::HandleMsg::Mint {
            recipient: env.contract.address.to_string(),
            amount: rewards_to_claim,
            padding: None,
            memo: None,
        };
        messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: state.erth_token_contract.to_string(),
            code_hash: state.erth_token_hash.clone(),
            msg: to_binary(&mint_msg)?,
            funds: vec![],
        }));

        let send_msg = if let Some(recieve_hash) = &allocation.recieve_hash {
            snip20::HandleMsg::Send {
                recipient: allocation.recieve_addr.to_string(),
                recipient_code_hash: Some(recieve_hash.clone()),
                amount: rewards_to_claim,
                msg: Some(to_binary(&SendMsg::AllocationSend { allocation_id })?),
                padding: None,
                memo: None,
            }
        } else {
            return Err(StdError::generic_err(
                "Missing recipient code hash for allocation",
            ));
        };

        messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: state.erth_token_contract.to_string(),
            code_hash: state.erth_token_hash.clone(),
            msg: to_binary(&send_msg)?,
            funds: vec![],
        }));
    } else {
        // Mint directly to the allocation receiver address
        let mint_msg = snip20::HandleMsg::Mint {
            recipient: allocation.recieve_addr.to_string(),
            amount: rewards_to_claim,
            padding: None,
            memo: None,
        };
        messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: state.erth_token_contract.to_string(),
            code_hash: state.erth_token_hash.clone(),
            msg: to_binary(&mint_msg)?,
            funds: vec![],
        }));
    };

    // Reset the accumulated rewards to zero
    allocation.accumulated_rewards = Uint128::zero();
    // Update the claim time
    allocation.last_claim = env.block.time;
    ALLOCATION_OPTIONS.save(deps.storage, &allocation_options)?;

    // Return the response with the mint message
    Ok(Response::new()
        .add_messages(messages)
        .add_attribute("action", "claim_allocation")
        .add_attribute("allocation_id", allocation_id.to_string())
        .add_attribute("rewards_claimed", rewards_to_claim.to_string()))
}

pub fn execute_add_allocation(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recieve_addr: Addr,
    recieve_hash: Option<String>,
    manager_addr: Option<Addr>,
    claimer_addr: Option<Addr>,
    use_send: bool,
) -> StdResult<Response> {
    // Load the current state
    let mut state = STATE.load(deps.storage)?;

    // Check if the sender is the contract manager
    if info.sender != state.contract_manager {
        return Err(StdError::generic_err(
            "Unauthorized: Only the contract manager can add an allocation",
        ));
    }

    // Load allocation options
    let mut allocation_options = ALLOCATION_OPTIONS.load(deps.storage)?;

    state.allocation_counter += 1;

    // Create a new allocation
    let allocation = Allocation {
        allocation_id: state.allocation_counter,
        recieve_addr,
        recieve_hash,
        manager_addr,
        claimer_addr,
        use_send,
        amount_allocated: Uint128::zero(),
        last_claim: env.block.time,
        accumulated_rewards: Uint128::zero(),
    };

    // Add the new allocation to the list
    allocation_options.push(allocation);

    // Save the updated allocation options and state
    ALLOCATION_OPTIONS.save(deps.storage, &allocation_options)?;
    STATE.save(deps.storage, &state)?;

    Ok(Response::default())
}

pub fn execute_withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    amount: Uint128,
) -> StdResult<Response> {
    // Load user info or return an error if no deposit is found
    let mut user_info: UserInfo = USER_INFO
        .get(deps.storage, &info.sender)
        .ok_or_else(|| StdError::generic_err("No deposit found"))?;

    // Ensure the user has enough funds to withdraw
    if user_info.staked_amount < amount {
        return Err(StdError::generic_err("Insufficient funds"));
    }

    // Calculate the new deposit amount after withdrawal
    let new_deposit_amount = user_info.staked_amount - amount;

    // Load allocation options and state
    let mut allocation_options = ALLOCATION_OPTIONS.load(deps.storage)?;
    let mut state = STATE.load(deps.storage)?;

    // Subtract the old allocations using the helper function
    subtract_old_allocations(
        &user_info.allocations,
        &mut allocation_options,
        &mut state,
    );

    if new_deposit_amount > Uint128::zero() {
        // If there's still a deposit left, recalculate allocations
        let new_allocations = add_new_allocations(
            new_deposit_amount,
            &user_info.percentages,
            &mut allocation_options,
            &mut state,
        )?;
        user_info.allocations = new_allocations;
        user_info.staked_amount = new_deposit_amount;

        // Save the updated user info back to storage
        USER_INFO.insert(deps.storage, &info.sender, &user_info)?;
    } else {
        // If the new deposit amount is zero, remove the user's info from storage
        USER_INFO.remove(deps.storage, &info.sender)?;
    }

    // Save the updated allocation options and state
    ALLOCATION_OPTIONS.save(deps.storage, &allocation_options)?;
    
    // Subtract the withdrawn amount from total deposits in state
    state.total_staked -= amount;
    
    // Save the updated state AFTER modifying total_staked
    STATE.save(deps.storage, &state)?;

    // Add the unbonding entry to UNBONDING_INFO
    let unbonding_period = 21 * 24 * 60 * 60; // 21 days in seconds
    let unbonding_time = Timestamp::from_seconds(env.block.time.seconds() + unbonding_period);
    let unbonding_entry = UnbondingEntry {
        amount,
        unbonding_time,
    };

    let mut unbonding_entries =
        UNBONDING_INFO.get(deps.storage, &info.sender).unwrap_or_else(Vec::new);
    unbonding_entries.push(unbonding_entry);
    UNBONDING_INFO.insert(deps.storage, &info.sender, &unbonding_entries)?;

    // No tokens are transferred at this time

    Ok(Response::new()
        .add_attribute("action", "request_withdraw")
        .add_attribute("amount", amount.to_string())
        .add_attribute("unbonding_time", unbonding_time.seconds().to_string()))
}

pub fn execute_claim_unbonded(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> StdResult<Response> {
    // Load unbonding entries for the user
    let mut unbonding_entries =
        UNBONDING_INFO.get(deps.storage, &info.sender).unwrap_or_else(Vec::new);

    // Filter unbonding entries that are ready to be claimed
    let mut claimable_amount = Uint128::zero();
    let current_time = env.block.time.seconds();
    unbonding_entries.retain(|entry| {
        if entry.unbonding_time.seconds() <= current_time {
            claimable_amount += entry.amount;
            false // Remove this entry
        } else {
            true // Keep this entry
        }
    });

    if claimable_amount == Uint128::zero() {
        return Err(StdError::generic_err("No tokens available to claim"));
    }

    // Save the updated unbonding entries back to storage
    UNBONDING_INFO.insert(deps.storage, &info.sender, &unbonding_entries)?;

    // Prepare and send the transfer message
    let state = STATE.load(deps.storage)?;
    let msg = snip20::HandleMsg::Transfer {
        recipient: info.sender.clone().to_string(),
        amount: claimable_amount,
        padding: None,
        memo: None,
    };
    let message = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: state.erth_token_contract.to_string(),
        code_hash: state.erth_token_hash,
        msg: to_binary(&msg)?,
        funds: vec![],
    });

    Ok(Response::new()
        .add_message(message)
        .add_attribute("action", "claim_unbonded")
        .add_attribute("amount", claimable_amount.to_string()))
}

// Add a helper function for distribution
fn distribute_allocation_rewards(
    deps: &mut DepsMut,
    current_time: Timestamp,
    last_upkeep: Timestamp,
) -> StdResult<(Uint128, u64, bool)> {
    // Constants for rewards
    let reward_rate_per_second: Uint128 = Uint128::from(1_000_000u128); // 1,000,000 ERTH per second (1 ERTH)
    
    // Calculate time elapsed since last upkeep
    let time_elapsed = current_time.seconds() - last_upkeep.seconds();
    
    // If no time has elapsed, return early
    if time_elapsed == 0 {
        return Ok((Uint128::zero(), 0, false));
    }
    
    // Load allocation options
    let mut allocation_options = ALLOCATION_OPTIONS.load(deps.storage)?;
    
    // If there are no allocations, return early
    if allocation_options.is_empty() {
        return Ok((Uint128::zero(), time_elapsed, false));
    }
    
    // Calculate total rewards for the period
    let total_rewards_for_period = reward_rate_per_second * Uint128::from(time_elapsed);
    
    // Calculate total allocation amount from loaded options
    let calculated_total_allocations: Uint128 = allocation_options
        .iter()
        .fold(Uint128::zero(), |acc, allocation| acc + allocation.amount_allocated);
    
    // If total is zero, return early
    if calculated_total_allocations.is_zero() {
        return Ok((Uint128::zero(), time_elapsed, false));
    }
    
    // Distribute rewards to each allocation based on their proportion of the total
    for allocation in allocation_options.iter_mut() {
        // Calculate this allocation's share of rewards
        let allocation_share = allocation.amount_allocated
            * total_rewards_for_period
            / calculated_total_allocations;
        
        // Add to accumulated rewards
        allocation.accumulated_rewards += allocation_share;
    }
    
    // Save the updated allocations
    ALLOCATION_OPTIONS.save(deps.storage, &allocation_options)?;
    
    // Return the total rewards distributed, time elapsed, and success flag
    Ok((total_rewards_for_period, time_elapsed, true))
}

// Update the execute_distribute_allocation_rewards function
pub fn execute_distribute_allocation_rewards(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> StdResult<Response> {
    // Load the current state
    let mut state = STATE.load(deps.storage)?;

    // Check if the sender is the contract manager
    if info.sender != state.contract_manager {
        return Err(StdError::generic_err(
            "Unauthorized: Only the contract manager can distribute allocation rewards",
        ));
    }

    // Use the helper function to distribute rewards
    let (total_rewards, time_elapsed, distribution_performed) = 
        distribute_allocation_rewards(&mut deps, env.block.time, state.last_upkeep)?;
    
    // If no time elapsed, return error
    if time_elapsed == 0 {
        return Err(StdError::generic_err("No time has elapsed since last upkeep"));
    }

    // Update the last upkeep time
    state.last_upkeep = env.block.time;
    STATE.save(deps.storage, &state)?;

    // Prepare the response based on whether distribution was performed
    let mut response = Response::new()
        .add_attribute("action", "distribute_allocation_rewards")
        .add_attribute("time_elapsed", time_elapsed.to_string());
    
    if distribution_performed {
        response = response.add_attribute("total_rewards_distributed", total_rewards.to_string());
    } else {
        response = response.add_attribute("result", "no allocations to update");
    }

    Ok(response)
}

// Update the execute_claim_staking_rewards function
pub fn execute_claim_staking_rewards(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> StdResult<Response> {
    // First, load the current state
    let mut state = STATE.load(deps.storage)?;
    
    // Use the helper function to distribute allocation rewards
    let (total_rewards, time_elapsed, distribution_performed) = 
        distribute_allocation_rewards(&mut deps, env.block.time, state.last_upkeep)?;
    
    // Update the last upkeep time if distribution was attempted
    if time_elapsed > 0 {
        state.last_upkeep = env.block.time;
        STATE.save(deps.storage, &state)?;
    }
    
    // Now continue with the normal staking rewards claim processing
    // Constants for rewards
    let reward_rate_per_second: Uint128 = Uint128::from(1_000_000u128); // 1,000,000 ERTH per second

    let mut user_info = USER_INFO
        .get(deps.storage, &info.sender)
        .ok_or_else(|| StdError::generic_err("no user info found"))?;
    let claim_time_elapsed = env.block.time.seconds() - user_info.last_claim.seconds();

    if claim_time_elapsed > 0 {
        // Calculate the staker's share of rewards
        let staker_share = user_info.staked_amount
            * reward_rate_per_second
            * Uint128::from(claim_time_elapsed)
            / state.total_staked;

        // Update the last reward time to the current time
        user_info.last_claim = env.block.time;
        USER_INFO.insert(deps.storage, &info.sender, &user_info)?;

        // Mint Staking Rewards
        let msg = snip20::HandleMsg::Mint {
            recipient: info.sender.clone().to_string(),
            amount: staker_share,
            memo: None,
            padding: None,
        };
        let message = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: state.erth_token_contract.to_string(),
            code_hash: state.erth_token_hash,
            msg: to_binary(&msg)?,
            funds: vec![],
        });

        // Create a response with all attributes
        let mut response = Response::new()
            .add_message(message)
            .add_attribute("action", "claim_staking_rewards")
            .add_attribute("staker", info.sender.to_string())
            .add_attribute("rewards_claimed", staker_share.to_string());
        
        // Add upkeep attributes if distribution was performed
        if distribution_performed {
            response = response
                .add_attribute("upkeep_performed", "true")
                .add_attribute("upkeep_time_elapsed", time_elapsed.to_string())
                .add_attribute("upkeep_rewards_distributed", total_rewards.to_string());
        }

        Ok(response)
    } else {
        Err(StdError::generic_err("No rewards available to claim"))
    }
}

pub fn execute_set_allocation(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    percentages: Vec<AllocationPercentage>,
) -> StdResult<Response> {
    // Load user info or return an error if no deposit is found
    let mut user_info: UserInfo = USER_INFO
        .get(deps.storage, &info.sender)
        .ok_or_else(|| StdError::generic_err("No deposit found"))?;

    // Load allocation options and state
    let mut allocation_options = ALLOCATION_OPTIONS.load(deps.storage)?;
    let mut state = STATE.load(deps.storage)?;

    // Subtract the old allocations using the helper function
    subtract_old_allocations(
        &user_info.allocations,
        &mut allocation_options,
        &mut state,
    );

    // Add the new allocations using the helper function
    let new_allocations = add_new_allocations(
        user_info.staked_amount,
        &percentages,
        &mut allocation_options,
        &mut state,
    )?;

    // Update the user's info with the new allocations and percentages
    user_info.allocations = new_allocations;
    user_info.percentages = percentages;

    // Save the updated user info back to storage
    USER_INFO.insert(deps.storage, &info.sender, &user_info)?;

    // Save the updated allocation options and state
    ALLOCATION_OPTIONS.save(deps.storage, &allocation_options)?;
    STATE.save(deps.storage, &state)?;

    Ok(Response::default())
}

pub fn try_receive(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    _sender: Addr,
    from: Addr,
    amount: Uint128,
    msg: Binary,
) -> Result<Response, StdError> {
    let msg: ReceiveMsg = from_binary(&msg)?;

    let state = STATE.load(deps.storage)?;
    if info.sender != state.erth_token_contract {
        return Err(StdError::generic_err("invalid snip"));
    }

    match msg {
        ReceiveMsg::StakeErth {} => receive_stake(deps, env, from, amount),
    }
}

pub fn receive_stake(
    deps: DepsMut,
    env: Env,
    from: Addr,
    amount: Uint128,
) -> StdResult<Response> {
    // Load the current state
    let mut state = STATE.load(deps.storage)?;

    // Fetch existing user information or initialize it if not present
    let user_info: UserInfo = match USER_INFO.get(deps.storage, &from) {
        Some(mut existing_user_info) => {
            // Load allocation options
            let mut allocation_options = ALLOCATION_OPTIONS.load(deps.storage)?;

            // Subtract old allocations
            subtract_old_allocations(
                &existing_user_info.allocations,
                &mut allocation_options,
                &mut state,
            );

            // Calculate the new total deposit amount
            existing_user_info.staked_amount += amount;

            // Calculate new allocations using the helper function
            let new_allocations = add_new_allocations(
                existing_user_info.staked_amount,
                &existing_user_info.percentages,
                &mut allocation_options,
                &mut state,
            )?;

            // Update the user information with new allocations
            existing_user_info.allocations = new_allocations;

            // Save the updated allocation options
            ALLOCATION_OPTIONS.save(deps.storage, &allocation_options)?;

            existing_user_info
        }
        None => {
            // Initialize new user info if not present
            UserInfo {
                staked_amount: amount, // Directly set the new deposit amount
                last_claim: env.block.time,
                allocations: Vec::new(),
                percentages: Vec::new(),
            }
        }
    };

    // Save the updated user information to storage
    USER_INFO.insert(deps.storage, &from, &user_info)?;

    // Update the total staked amount in state
    state.total_staked += amount;
    STATE.save(deps.storage, &state)?;

    Ok(Response::default())
}

#[entry_point]
pub fn migrate(deps: DepsMut, env: Env, msg: MigrateMsg) -> StdResult<Response> {
    match msg {
        MigrateMsg::Migrate {} => {
            // Load the current state
            let state = STATE.load(deps.storage)?;
            


            // Register the contract as a receiver for the ERTH token
            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: state.erth_token_contract.to_string(),
                code_hash: state.erth_token_hash.clone(),
                msg: to_binary(&snip20::HandleMsg::RegisterReceive {
                    code_hash: env.contract.code_hash.clone(),
                    padding: None,
                })?,
                funds: vec![],
            });
            
            Ok(Response::new()
                .add_message(message)
                .add_attribute("action", "migrate")
            )
        }
    }
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetState {} => to_binary(&query_state(deps)?),
        QueryMsg::GetAllocationOptions {} => to_binary(&query_allocation_options(deps)?),
        QueryMsg::GetUserInfo { address } => to_binary(&query_user_info(deps, env, address)?),
    }
}

fn query_state(deps: Deps) -> StdResult<StateResponse> {
    let state = STATE.load(deps.storage)?;
    Ok(StateResponse { state })
}

fn query_allocation_options(deps: Deps) -> StdResult<AllocationOptionResponse> {
    let allocations = ALLOCATION_OPTIONS.load(deps.storage)?;
    Ok(AllocationOptionResponse { allocations })
}

pub fn query_user_info(deps: Deps, env: Env, address: Addr) -> StdResult<UserInfoResponse> {
    // Constants for rewards
    let reward_rate_per_second: Uint128 = Uint128::from(1_000_000u128); // 1,000,000 ERTH per second

    // Load the current state
    let state = STATE.load(deps.storage)?;

    // Load user info or use default if not found
    let user_info = USER_INFO
        .get(deps.storage, &address)
        .unwrap_or(UserInfo {
            staked_amount: Uint128::zero(),
            last_claim: env.block.time,
            allocations: Vec::new(),
            percentages: Vec::new(),
        });

    // Calculate the time elapsed since the last claim
    let time_elapsed = env.block.time.seconds() - user_info.last_claim.seconds();

    // Calculate the staking rewards due
    let staking_rewards_due = if time_elapsed > 0 {
        user_info.staked_amount
            * reward_rate_per_second
            * Uint128::from(time_elapsed)
            / state.total_staked
    } else {
        Uint128::zero() // No rewards if no time has passed since the last claim
    };

    // Load unbonding entries
    let unbonding_entries = UNBONDING_INFO
        .get(deps.storage, &address)
        .unwrap_or_else(Vec::new);

    // Prepare the response
    let user_info_response = UserInfoResponse {
        user_info,
        staking_rewards_due,
        total_staked: state.total_staked,
        unbonding_entries,
    };

    Ok(user_info_response)
}

// Helper functions

pub fn subtract_old_allocations(
    old_allocations: &[UserAllocation],
    allocation_options: &mut [Allocation],
    state: &mut State,
) {
    for old_allocation in old_allocations {
        for allocation_option in allocation_options.iter_mut() {
            if old_allocation.allocation_id == allocation_option.allocation_id {
                allocation_option.amount_allocated -= old_allocation.amount_allocated;
                state.total_allocations -= old_allocation.amount_allocated;
            }
        }
    }
}

pub fn add_new_allocations(
    deposit_amount: Uint128,
    percentages: &[AllocationPercentage],
    allocation_options: &mut [Allocation],
    state: &mut State,
) -> StdResult<Vec<UserAllocation>> {
    // Check for duplicate allocation IDs
    let mut seen_ids = std::collections::HashSet::new();
    for percentage in percentages {
        if !seen_ids.insert(percentage.allocation_id) {
            return Err(StdError::generic_err(
                "Duplicate allocation ID found. Each allocation ID must be unique.",
            ));
        }
        
        // Check that each allocation ID exists in the available options
        let allocation_exists = allocation_options.iter().any(|allocation| {
            allocation.allocation_id == percentage.allocation_id
        });
        
        if !allocation_exists {
            return Err(StdError::generic_err(
                format!("Allocation ID {} does not exist", percentage.allocation_id),
            ));
        }
    }

    let mut new_allocations: Vec<UserAllocation> = Vec::new();
    let mut total_percentage = Uint128::zero();

    for percentage in percentages {
        if percentage.percentage > Uint128::zero() {
            for allocation in allocation_options.iter_mut() {
                // Assuming both AllocationPercentage and Allocation have an `allocation_id` field
                if percentage.allocation_id == allocation.allocation_id {
                    let allocation_amount =
                        deposit_amount * percentage.percentage / Uint128::from(100u32);
                    allocation.amount_allocated += allocation_amount;
                    state.total_allocations += allocation_amount;
                    total_percentage += percentage.percentage;

                    let user_allocation = UserAllocation {
                        allocation_id: allocation.allocation_id, // Unique ID for this allocation
                        amount_allocated: allocation_amount,
                    };
                    new_allocations.push(user_allocation);
                }
            }
        }
    }

    // Ensure that the total percentages add up to 100%
    if total_percentage != Uint128::from(100u32) {
        return Err(StdError::generic_err(
            "Percentage error: allocations must sum to 100%",
        ));
    }

    Ok(new_allocations)
}
