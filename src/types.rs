use soroban_sdk::contracttype;

/// Basis points: 0 to 10000 (0% to 100%).
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Bps(pub u32);

/// Unique identifier for a revenue period.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct PeriodId(pub u64);

/// Amount of revenue in some token (i128-based).
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct RevenueAmount(pub i128);

impl From<u32> for Bps {
    fn from(val: u32) -> Self {
        Bps(val)
    }
}

impl From<Bps> for u32 {
    fn from(bps: Bps) -> Self {
        bps.0
    }
}

impl From<u64> for PeriodId {
    fn from(val: u64) -> Self {
        PeriodId(val)
    }
}

impl From<PeriodId> for u64 {
    fn from(pid: PeriodId) -> Self {
        pid.0
    }
}

impl From<i128> for RevenueAmount {
    fn from(val: i128) -> Self {
        RevenueAmount(val)
    }
}

impl From<RevenueAmount> for i128 {
    fn from(amount: RevenueAmount) -> Self {
        amount.0
    }
}
