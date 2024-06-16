use cosmwasm_std::{
    entry_point, to_binary, from_binary, Binary, Deps, DepsMut, Env,
    MessageInfo, Response, StdError, StdResult, Addr, Uint128, CosmosMsg,
    WasmMsg,
};

use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg, StateResponse, Snip20Msg, StakeResponse,
    ReceiveMsg, AllocationPercentage, AllocationResponse, AllocationOptionResponse,
};
use crate::state::{STATE, State, DEPOSIT_AMOUNTS, ALLOCATION_OPTIONS, INDIVIDUAL_ALLOCATIONS, 
    Allocation, INDIVIDUAL_PERCENTAGES,
};


#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, StdError> {
    let state = State {
        erth_contract: msg.erth_contract,
        erth_hash: msg.erth_hash,
        total_deposits: Uint128::zero(),
        total_allocations: Uint128::zero(),
    };
    STATE.save(deps.storage, &state)?;

    let allocation_options = Vec::new();
    ALLOCATION_OPTIONS.save(deps.storage, &allocation_options)?;

    let msg = to_binary(&Snip20Msg::register_receive(env.contract.code_hash))?;
    let message = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: state.erth_contract.into_string(),
        code_hash: state.erth_hash,
        msg,
        funds: vec![],
    });
    Ok(Response::new().add_message(message))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, StdError> {
    match msg {
        ExecuteMsg::Withdraw {amount} => try_withdraw(deps, env, info, amount),
        ExecuteMsg::SetAllocation {percentages} => try_set_allocation(deps, env, info, percentages),
        ExecuteMsg::AddAllocationOption{address} => try_add_allocation_option(deps, env, info, address),
        ExecuteMsg::Faucet{} => try_faucet(deps, env, info),
        ExecuteMsg::Receive {
            sender,
            from,
            amount,
            msg,
            memo: _,
        } => try_receive(deps, env, info, sender, from, amount, msg),  
    }
}


pub fn try_add_allocation_option(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    address: Addr,
) -> StdResult<Response> {

    // load allocation options
    let mut allocation_options = ALLOCATION_OPTIONS.load(deps.storage)?;
    // Check if there is a matching contract in the allocation options
    for allocation_option in &allocation_options {
        if address == allocation_option.address {
            return Err(StdError::generic_err("Option already exists"));
        }
    }
    let allocation = Allocation {
        address: address,
        amount: Uint128::zero(),
    };
    allocation_options.push(allocation);
    ALLOCATION_OPTIONS.save(deps.storage, &allocation_options)?;
    Ok(Response::default())
}

pub fn try_withdraw(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    amount: Uint128,
) -> StdResult<Response> {

    // Check if there is a deposit
    let already_deposited_option: Option<Uint128> = DEPOSIT_AMOUNTS.get(deps.storage, &info.sender);
    let new_deposit_amount = match already_deposited_option {
        Some(existing_amount) => {
            if existing_amount < amount {
                return Err(StdError::generic_err("Insufficient funds"));
            }
            existing_amount - amount // Subtract the amount
        },
        None => return Err(StdError::generic_err("No deposit found")),
    };

    let mut state = STATE.load(deps.storage)?;
    // Check if there is an existing allocation and remove it
    if let Some(individual_allocations) = INDIVIDUAL_ALLOCATIONS.get(deps.storage, &info.sender) {
        // Load allocation options
        let mut allocation_options = ALLOCATION_OPTIONS.load(deps.storage)?;
        for old_allocation in &individual_allocations {
            // Check if there is a matching contract in the allocation options
            for allocation_option in allocation_options.iter_mut() {
                if old_allocation.address == allocation_option.address {
                    allocation_option.amount -= old_allocation.amount;
                    state.total_allocations -= old_allocation.amount;
                }
            }
        }

        // Set new allocation if there's still some deposit left
        if new_deposit_amount > Uint128::zero() {
            if let Some(percentages) = INDIVIDUAL_PERCENTAGES.get(deps.storage, &info.sender) {
                let mut new_user_allocations: Vec<Allocation> = Vec::new();
                for percentage in &percentages {
                    if percentage.percentage > Uint128::zero() {
                        for allocation_option in allocation_options.iter_mut() {
                            if percentage.address == allocation_option.address {
                                let allocation_amount = new_deposit_amount * percentage.percentage / Uint128::from(100u32);
                                allocation_option.amount += allocation_amount;
                                state.total_allocations += allocation_amount;

                                let allocation = Allocation {
                                    address: allocation_option.address.clone(),
                                    amount: allocation_amount,
                                };
                                new_user_allocations.push(allocation);
                            }
                        }
                    }
                }

                // Save the updated individual allocations
                INDIVIDUAL_ALLOCATIONS.insert(deps.storage, &info.sender, &new_user_allocations)?;
            }
        } else {
            // If the new deposit amount is zero, remove the individual allocations and percentages
            INDIVIDUAL_ALLOCATIONS.remove(deps.storage, &info.sender)?;
            INDIVIDUAL_PERCENTAGES.remove(deps.storage, &info.sender)?;
        }
        // Save the updated allocation options
        ALLOCATION_OPTIONS.save(deps.storage, &allocation_options)?;
    }


    
    // Save the new deposit amount to storage
    DEPOSIT_AMOUNTS.insert(deps.storage, &info.sender, &new_deposit_amount)?;

    // Subtract the withdrawn amount from total deposits in state
    state.total_deposits -= amount;
    STATE.save(deps.storage, &state)?;

    // Prepare and send the transfer message
    let msg = to_binary(&Snip20Msg::transfer_snip(
        info.sender.clone(),
        amount,
    ))?;
    let message = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: state.erth_contract.to_string(),
        code_hash: state.erth_hash,
        msg,
        funds: vec![],
    });

    let response = Response::new().add_message(message);
    Ok(response)
}


pub fn try_set_allocation(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    percentages: Vec<AllocationPercentage>,
) -> StdResult<Response> {

    // check if there is a deposit
    let deposit_amount = match DEPOSIT_AMOUNTS.get(deps.storage, &info.sender) {
        Some(amount) => amount,
        None => return Err(StdError::generic_err("No deposit found")),
    };
    // load allocation options
    let mut allocation_options = ALLOCATION_OPTIONS.load(deps.storage)?;
    let mut state = STATE.load(deps.storage)?;
    // check if there is an existing allocation and remove it
    if let Some(individual_allocations) = INDIVIDUAL_ALLOCATIONS.get(deps.storage, &info.sender) {
        for old_allocation in individual_allocations {
            // Check if there is a matching contract in the allocation options
            for allocation_option in allocation_options.iter_mut() {
                if old_allocation.address == allocation_option.address {
                    allocation_option.amount -= old_allocation.amount;
                    state.total_allocations -= old_allocation.amount;
                }
            }
        }
    }
    // set new allocation
    let mut completed_percentage = Uint128::zero();
    let mut new_user_allocations: Vec<Allocation> = Vec::new();
    for percentage in &percentages {
        if percentage.percentage > Uint128::zero() {
            for allocation_option in allocation_options.iter_mut() {
                if percentage.address == allocation_option.address {
                    let allocation_amount = deposit_amount * percentage.percentage /  Uint128::from(100u32);
                    allocation_option.amount += allocation_amount;
                    state.total_allocations += allocation_amount;
                    completed_percentage += percentage.percentage;
                    let allocation = Allocation {
                        address: allocation_option.address.clone(),
                        amount: allocation_amount.clone(),
                    };
                    new_user_allocations.push(allocation);

                }
            }
        }
    }
    if completed_percentage != Uint128::from(100u32) {
        return Err(StdError::generic_err("percentage error"))
    }
    // save individual allocation to storage
    INDIVIDUAL_ALLOCATIONS.insert(deps.storage, &info.sender, &new_user_allocations)?;
    INDIVIDUAL_PERCENTAGES.insert(deps.storage, &info.sender, &percentages)?;
    // save total allocations to storage
    ALLOCATION_OPTIONS.save(deps.storage, &allocation_options)?;
    STATE.save(deps.storage, &state)?;
    Ok(Response::default())
}

pub fn try_faucet(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> StdResult<Response> {

    let state = STATE.load(deps.storage)?;
     // Create the contract execution message
    let msg = to_binary(&Snip20Msg::mint_msg(
        info.sender.clone(),
        Uint128::from(1000000u32),
    ))?;
    let execute_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: state.erth_contract.to_string(),
        code_hash: state.erth_hash.to_string(),
        funds: vec![],
        msg: msg,
    });
    // Return the execution message in the Response
    let response = Response::new()
    .add_message(execute_msg);
    Ok(response)
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
    if info.sender != state.erth_contract {
        return Err(StdError::generic_err("invalid snip"));
    }

    match msg {
        ReceiveMsg::Deposit {} => try_deposit(deps, env, from, amount),
    }   
}

pub fn try_deposit(
    deps: DepsMut,
    _env: Env,
    from: Addr,
    amount: Uint128,
) -> StdResult<Response> {
    // check if there is already a deposit under address
    let already_deposited_option: Option<Uint128> = DEPOSIT_AMOUNTS.get(deps.storage, &from);
    let mut state = STATE.load(deps.storage)?;

    match already_deposited_option {
        Some(existing_amount) => {
            // Calculate the new total deposit amount
            let new_deposit_amount = existing_amount + amount;

            // Fetch the existing allocations for this user
            if let Some(individual_allocations) = INDIVIDUAL_ALLOCATIONS.get(deps.storage, &from) {
                // Subtract the old allocations from the allocation options and state
                let mut allocation_options = ALLOCATION_OPTIONS.load(deps.storage)?;
                for allocation in &individual_allocations {
                    for allocation_option in allocation_options.iter_mut() {
                        if allocation.address == allocation_option.address {
                            allocation_option.amount -= allocation.amount;
                            state.total_allocations -= allocation.amount;
                        }
                    }
                }

                // Calculate new allocations based on the new deposit amount
                let percentages = INDIVIDUAL_PERCENTAGES.get(deps.storage, &from)
                    .ok_or_else(|| StdError::generic_err("No percentages found for the deposit"))?;
                
                let mut new_allocations: Vec<Allocation> = Vec::new();
                for percentage in &percentages {
                    if percentage.percentage > Uint128::zero() {
                        for allocation_option in allocation_options.iter_mut() {
                            if percentage.address == allocation_option.address {
                                let allocation_amount = new_deposit_amount * percentage.percentage / Uint128::from(100u32);
                                allocation_option.amount += allocation_amount;
                                state.total_allocations += allocation_amount;

                                let allocation = Allocation {
                                    address: allocation_option.address.clone(),
                                    amount: allocation_amount,
                                };
                                new_allocations.push(allocation);
                            }
                        }
                    }
                }

                // Save the updated individual allocations
                INDIVIDUAL_ALLOCATIONS.insert(deps.storage, &from, &new_allocations)?;

                // Save the updated allocation options
                ALLOCATION_OPTIONS.save(deps.storage, &allocation_options)?;
            }

            // Update the deposit amount in storage
            DEPOSIT_AMOUNTS.insert(deps.storage, &from, &new_deposit_amount)?;

            // Update the total deposits in state
            state.total_deposits += amount;
            STATE.save(deps.storage, &state)?;
        }
        None => {
            // If no existing amount, use the new amount directly
            DEPOSIT_AMOUNTS.insert(deps.storage, &from, &amount)?;
            state.total_deposits += amount;
            STATE.save(deps.storage, &state)?;
        }
    };

    Ok(Response::default())
}


#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetState {} => to_binary(&query_state(deps)?),
        QueryMsg::GetAllocationOptions {} => to_binary(&query_allocation_options(deps)?),
        QueryMsg::GetAllocation{address} => to_binary(&query_allocation(deps, address)?),
        QueryMsg::GetStake{address} => to_binary(&query_stake(deps, address)?),
    }
}

fn query_state(deps: Deps) -> StdResult<StateResponse> {
    let state = STATE.load(deps.storage)?;
    Ok(StateResponse { state: state })
}

fn query_allocation_options(deps: Deps) -> StdResult<AllocationOptionResponse> {
    let allocations = ALLOCATION_OPTIONS.load(deps.storage)?;
    Ok(AllocationOptionResponse { allocations: allocations })
}

fn query_allocation(deps: Deps, address: Addr,) -> StdResult<AllocationResponse> {

    // Check if there is an allocation under the address
    let individual_percentages = INDIVIDUAL_PERCENTAGES
    .get(deps.storage, &address)
    .unwrap_or_else(Vec::new);

    let individual_allocations = INDIVIDUAL_ALLOCATIONS
    .get(deps.storage, &address)
    .unwrap_or_else(Vec::new);

    Ok(AllocationResponse { 
        percentages: individual_percentages,
        allocations: individual_allocations,
     })
}

fn query_stake(deps: Deps, address: Addr,) -> StdResult<StakeResponse> {

// Check if there is a deposit under the address
let deposit = DEPOSIT_AMOUNTS
    .get(deps.storage, &address)
    .unwrap_or_else(|| Uint128::zero());


    Ok(StakeResponse { 
        amount: deposit,
     })
}