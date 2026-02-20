use soroban_sdk::{Address, Env, Map, Vec};
use crate::types::DataKey;
use crate::events::{EVENT_BL_ADD, EVENT_BL_REM};

pub fn blacklist_add(env: &Env, caller: &Address, token: &Address, investor: &Address) {
    let key = DataKey::Blacklist(token.clone());
    let mut map: Map<Address, bool> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Map::new(env));

    map.set(investor.clone(), true);
    env.storage().persistent().set(&key, &map);

    env.events()
        .publish((EVENT_BL_ADD, token.clone(), caller.clone()), investor.clone());
}

pub fn blacklist_remove(env: &Env, caller: &Address, token: &Address, investor: &Address) {
    let key = DataKey::Blacklist(token.clone());
    let mut map: Map<Address, bool> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Map::new(env));

    map.remove(investor.clone());
    env.storage().persistent().set(&key, &map);

    env.events()
        .publish((EVENT_BL_REM, token.clone(), caller.clone()), investor.clone());
}

pub fn is_blacklisted(env: &Env, token: &Address, investor: &Address) -> bool {
    let key = DataKey::Blacklist(token.clone());
    env.storage()
        .persistent()
        .get::<DataKey, Map<Address, bool>>(&key)
        .map(|m| m.get(investor.clone()).unwrap_or(false))
        .unwrap_or(false)
}

pub fn get_blacklist(env: &Env, token: &Address) -> Vec<Address> {
    let key = DataKey::Blacklist(token.clone());
    env.storage()
        .persistent()
        .get::<DataKey, Map<Address, bool>>(&key)
        .map(|m| m.keys())
        .unwrap_or_else(|| Vec::new(env))
}