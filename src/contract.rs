use cosmwasm_std::{
    entry_point, to_binary, from_binary, Binary, Deps, DepsMut, Env,
    MessageInfo, Response, StdError, StdResult, Addr, Uint128, CosmosMsg,
    WasmMsg,
};

use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg, StateResponse, Snip20Msg,
    ReceiveMsg, AllocationPercentage, AllocationResponse,
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
        ExecuteMsg::AddAllocationOption{contract, hash} => try_add_allocation_option(deps, env, info, contract, hash),
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
    contract: Addr,
    hash: String,
) -> StdResult<Response> {

    // load allocation options
    let mut allocation_options = ALLOCATION_OPTIONS.load(deps.storage)?;
    // Check if there is a matching contract in the allocation options
    for allocation_option in &allocation_options {
        if contract == allocation_option.contract {
            return Err(StdError::generic_err("Option already exists"));
        }
    }
    let allocation = Allocation {
        contract: contract,
        hash: hash,
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

    // check if there is a deposit
    let already_deposited_option:Option<Uint128> = DEPOSIT_AMOUNTS.get(deps.storage, &info.sender);
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
    // check if there is an existing allocation and remove it
    if let Some(individual_allocations) = INDIVIDUAL_ALLOCATIONS.get(deps.storage, &info.sender) {
        // load allocation options
        let mut allocation_options = ALLOCATION_OPTIONS.load(deps.storage)?;
        for old_allocation in individual_allocations {
            // Check if there is a matching contract in the allocation options
            for allocation_option in allocation_options.iter_mut() {
                if old_allocation.contract == allocation_option.contract {
                    allocation_option.amount -= old_allocation.amount;
                    state.total_allocations -= old_allocation.amount;
                }
            }
        }
        // set new allocation
        if new_deposit_amount > Uint128::zero() {
            if let Some(percentages) = INDIVIDUAL_PERCENTAGES.get(deps.storage, &info.sender) {
                let mut completed_percentage = Uint128::zero();
                let mut new_user_allocations: Vec<Allocation> = Vec::new();
                for percentage in percentages {
                    for allocation_option in allocation_options.iter_mut() {
                        if percentage.contract == allocation_option.contract {
                            let allocation_amount = new_deposit_amount / Uint128::from(100u32) * percentage.percentage;
                            allocation_option.amount += allocation_amount;
                            state.total_allocations += allocation_amount;
                            completed_percentage += percentage.percentage;
                            let allocation = Allocation {
                                contract: allocation_option.contract.clone(),
                                hash: allocation_option.hash.clone(),
                                amount: allocation_amount.clone(),
                            };
                            new_user_allocations.push(allocation);

                        }
                    }
                }
                // save individual allocation to storage
                INDIVIDUAL_ALLOCATIONS.insert(deps.storage, &info.sender, &new_user_allocations)?;
            }
        }
    }
    // save to deposit storage
    DEPOSIT_AMOUNTS.insert(deps.storage, &info.sender, &new_deposit_amount)?;
    // subtract deposit from total deposits in state
    state.total_deposits -= amount;
    STATE.save(deps.storage, &state)?;

    let msg = to_binary(&Snip20Msg::transfer_snip(
        info.sender,
        amount,
    ))?;
    let message = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: state.erth_contract.to_string(),
        code_hash: state.erth_hash,
        msg,
        funds: vec![],
    });
    let response = Response::new()
    .add_message(message);
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
                if old_allocation.contract == allocation_option.contract {
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
        for allocation_option in allocation_options.iter_mut() {
            if percentage.contract == allocation_option.contract {
                let allocation_amount = deposit_amount / Uint128::from(100u32) * percentage.percentage;
                allocation_option.amount += allocation_amount;
                state.total_allocations += allocation_amount;
                completed_percentage += percentage.percentage;
                let allocation = Allocation {
                    contract: allocation_option.contract.clone(),
                    hash: allocation_option.hash.clone(),
                    amount: allocation_amount.clone(),
                };
                new_user_allocations.push(allocation);

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
    let already_deposited_option:Option<Uint128> = DEPOSIT_AMOUNTS.get(deps.storage, &from);
    let new_deposit_amount = match already_deposited_option {
        Some(existing_amount) => existing_amount + amount,  // Add the new amount to the existing amount
        None => amount,  // If no existing amount, use the new amount directly
    };
    // load allocation options
    let mut allocation_options = ALLOCATION_OPTIONS.load(deps.storage)?;
    let mut state = STATE.load(deps.storage)?;
    // check if there is an existing allocation and remove it
    if let Some(individual_allocations) = INDIVIDUAL_ALLOCATIONS.get(deps.storage, &from) {
        for old_allocation in individual_allocations {
            // Check if there is a matching contract in the allocation options
            for allocation_option in allocation_options.iter_mut() {
                if old_allocation.contract == allocation_option.contract {
                    allocation_option.amount -= old_allocation.amount;
                    state.total_allocations -= old_allocation.amount;
                }
            }
        }
        // set new allocation
        if let Some(percentages) = INDIVIDUAL_PERCENTAGES.get(deps.storage, &from) {
            let mut completed_percentage = Uint128::zero();
            let mut new_user_allocations: Vec<Allocation> = Vec::new();
            for percentage in percentages {
                for allocation_option in allocation_options.iter_mut() {
                    if percentage.contract == allocation_option.contract {
                        let allocation_amount = new_deposit_amount / Uint128::from(100u32) * percentage.percentage;
                        allocation_option.amount += allocation_amount;
                        state.total_allocations += allocation_amount;
                        completed_percentage += percentage.percentage;
                        let allocation = Allocation {
                            contract: allocation_option.contract.clone(),
                            hash: allocation_option.hash.clone(),
                            amount: allocation_amount.clone(),
                        };
                        new_user_allocations.push(allocation);

                    }
                }
            }
            // save individual allocation to storage
            INDIVIDUAL_ALLOCATIONS.insert(deps.storage, &from, &new_user_allocations)?;
        }
    }
    // save to deposit storage
    DEPOSIT_AMOUNTS.insert(deps.storage, &from, &new_deposit_amount)?;
    // add deposit to total deposits in state
    state.total_deposits += amount;
    STATE.save(deps.storage, &state)?;
    Ok(Response::default())
}


#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetState {} => to_binary(&query_state(deps)?),
        QueryMsg::GetAllocation{address} => to_binary(&query_allocation(deps, address)?),
    }
}

fn query_state(deps: Deps) -> StdResult<StateResponse> {
    let state = STATE.load(deps.storage)?;
    Ok(StateResponse { state: state })
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
