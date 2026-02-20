use soroban_sdk::{symbol_short, Symbol};

pub const EVENT_REVENUE_REPORTED: Symbol = symbol_short!("rev_rep");
pub const EVENT_BL_ADD:           Symbol = symbol_short!("bl_add");
pub const EVENT_BL_REM:           Symbol = symbol_short!("bl_rem");
pub const EVENT_PAUSED:           Symbol = symbol_short!("paused");
pub const EVENT_RESUMED:          Symbol = symbol_short!("resumed");
pub const EVENT_CLOSED:           Symbol = symbol_short!("closed");