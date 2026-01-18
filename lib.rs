#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype,
    token::TokenInterface, Address, Env, MuxedAddress, String, Symbol,
};

// ===== Auxano (AVXO) constants =====
const TOKEN_NAME: &str = "Auxano";
const TOKEN_SYMBOL: &str = "AVXO";
const TOKEN_DECIMALS: u32 = 7;
const FIXED_SUPPLY_WHOLE: i128 = 77_000_000_000;

// ===== Errors =====
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TokenError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    AmountMustBePositive = 3,
    InsufficientBalance = 4,
    InsufficientAllowance = 5,
    BurnDisabled = 6,
}

// ===== Storage =====
#[contracttype]
pub enum DataKey {
    Inited,
    TotalSupply,
    MetaName,
    MetaSymbol,
    MetaDecimals,
    Balance(Address),
}

// ===== Events =====
#[contractevent]
pub struct TransferEvent {
    pub from: Address,
    pub to: Address,
    pub to_muxed_id: Option<u64>,
    pub amount: i128,
}

// ===== Allowances =====
#[contracttype]
#[derive(Clone)]
pub struct AllowanceKey {
    pub from: Address,
    pub spender: Address,
}

#[contracttype]
#[derive(Clone)]
pub struct AllowanceValue {
    pub amount: i128,
    pub expiration_ledger: u32,
}

// ===== Helpers =====
fn is_inited(e: &Env) -> bool {
    e.storage()
        .instance()
        .get::<DataKey, bool>(&DataKey::Inited)
        .unwrap_or(false)
}

fn require_inited(e: &Env) {
    if !is_inited(e) {
        soroban_sdk::panic_with_error!(e, TokenError::NotInitialized);
    }
}

fn read_balance(e: &Env, addr: &Address) -> i128 {
    e.storage()
        .persistent()
        .get::<DataKey, i128>(&DataKey::Balance(addr.clone()))
        .unwrap_or(0)
}

fn write_balance(e: &Env, addr: &Address, v: i128) {
    e.storage()
        .persistent()
        .set(&DataKey::Balance(addr.clone()), &v);
}

fn allowance_storage_key(e: &Env, from: &Address, spender: &Address) -> (Symbol, AllowanceKey) {
    (
        Symbol::new(e, "ALW"),
        AllowanceKey {
            from: from.clone(),
            spender: spender.clone(),
        },
    )
}

fn read_allowance(e: &Env, from: &Address, spender: &Address) -> AllowanceValue {
    let k = allowance_storage_key(e, from, spender);
    e.storage()
        .persistent()
        .get::<(Symbol, AllowanceKey), AllowanceValue>(&k)
        .unwrap_or(AllowanceValue {
            amount: 0,
            expiration_ledger: 0,
        })
}

fn write_allowance(e: &Env, from: &Address, spender: &Address, v: &AllowanceValue) {
    let k = allowance_storage_key(e, from, spender);
    e.storage()
        .persistent()
        .set::<(Symbol, AllowanceKey), AllowanceValue>(&k, v);
}

// ===== Contract =====
#[contract]
pub struct AuxanoToken;

#[contractimpl]
impl AuxanoToken {
    // Fixed supply: minted once to `recipient` at initialization.
    pub fn initialize(e: Env, recipient: Address) {
        if is_inited(&e) {
            soroban_sdk::panic_with_error!(&e, TokenError::AlreadyInitialized);
        }

        e.storage()
            .instance()
            .set(&DataKey::MetaName, &String::from_str(&e, TOKEN_NAME));
        e.storage()
            .instance()
            .set(&DataKey::MetaSymbol, &String::from_str(&e, TOKEN_SYMBOL));
        e.storage()
            .instance()
            .set(&DataKey::MetaDecimals, &TOKEN_DECIMALS);

        let scale: i128 = 10i128.pow(TOKEN_DECIMALS);
        let supply = FIXED_SUPPLY_WHOLE * scale;

        e.storage().instance().set(&DataKey::TotalSupply, &supply);
        write_balance(&e, &recipient, supply);

        e.storage().instance().set(&DataKey::Inited, &true);
    }

    pub fn total_supply(e: Env) -> i128 {
        require_inited(&e);
        e.storage()
            .instance()
            .get::<DataKey, i128>(&DataKey::TotalSupply)
            .unwrap()
    }
}

// ===== SEP-41 Interface =====
#[contractimpl]
impl TokenInterface for AuxanoToken {
    fn allowance(e: Env, from: Address, spender: Address) -> i128 {
        require_inited(&e);
        let v = read_allowance(&e, &from, &spender);
        let cur = e.ledger().sequence();
        if v.expiration_ledger != 0 && v.expiration_ledger < cur {
            0
        } else {
            v.amount
        }
    }

    fn approve(e: Env, from: Address, spender: Address, amount: i128, expiration_ledger: u32) {
        require_inited(&e);
        if amount < 0 {
            soroban_sdk::panic_with_error!(&e, TokenError::AmountMustBePositive);
        }

        // SEP-41 behavior: non-zero approvals must not already be expired
        let cur = e.ledger().sequence();
        if amount != 0 && expiration_ledger < cur {
            soroban_sdk::panic_with_error!(&e, TokenError::AmountMustBePositive);
        }

        from.require_auth();

        let v = AllowanceValue {
            amount,
            expiration_ledger,
        };
        write_allowance(&e, &from, &spender, &v);
    }

    fn balance(e: Env, id: Address) -> i128 {
        require_inited(&e);
        read_balance(&e, &id)
    }

    fn transfer(e: Env, from: Address, to: MuxedAddress, amount: i128) {
        require_inited(&e);
        if amount <= 0 {
            soroban_sdk::panic_with_error!(&e, TokenError::AmountMustBePositive);
        }

        from.require_auth();

        let to_addr = to.address();
        let bal = read_balance(&e, &from);
        if bal < amount {
            soroban_sdk::panic_with_error!(&e, TokenError::InsufficientBalance);
        }

        write_balance(&e, &from, bal - amount);
        write_balance(&e, &to_addr, read_balance(&e, &to_addr) + amount);

        TransferEvent {
            from,
            to: to_addr,
            to_muxed_id: to.id(),
            amount,
        }
        .publish(&e);
    }

    fn transfer_from(e: Env, spender: Address, from: Address, to: Address, amount: i128) {
        require_inited(&e);
        if amount <= 0 {
            soroban_sdk::panic_with_error!(&e, TokenError::AmountMustBePositive);
        }

        spender.require_auth();

        let mut v = read_allowance(&e, &from, &spender);
        let cur = e.ledger().sequence();

        if v.expiration_ledger != 0 && v.expiration_ledger < cur {
            soroban_sdk::panic_with_error!(&e, TokenError::InsufficientAllowance);
        }
        if v.amount < amount {
            soroban_sdk::panic_with_error!(&e, TokenError::InsufficientAllowance);
        }

        v.amount -= amount;
        write_allowance(&e, &from, &spender, &v);

        let bal = read_balance(&e, &from);
        if bal < amount {
            soroban_sdk::panic_with_error!(&e, TokenError::InsufficientBalance);
        }

        write_balance(&e, &from, bal - amount);
        write_balance(&e, &to, read_balance(&e, &to) + amount);
    }

    // Burns are intentionally disabled (BTC-style "only lost keys reduce circulation").
    fn burn(e: Env, _from: Address, _amount: i128) {
        require_inited(&e);
        soroban_sdk::panic_with_error!(&e, TokenError::BurnDisabled);
    }

    fn burn_from(e: Env, _spender: Address, _from: Address, _amount: i128) {
        require_inited(&e);
        soroban_sdk::panic_with_error!(&e, TokenError::BurnDisabled);
    }

    fn decimals(_: Env) -> u32 {
        TOKEN_DECIMALS
    }

    fn name(e: Env) -> String {
        String::from_str(&e, TOKEN_NAME)
    }

    fn symbol(e: Env) -> String {
        String::from_str(&e, TOKEN_SYMBOL)
    }
}
