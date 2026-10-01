//! Adversarial coverage for `AmountValidationMatrix::category_for_function` (#1031).
//!
//! `category_for_function` is a best-effort public lookup used by callers and
//! tests to discover which amount-validation rules apply to a contract
//! entrypoint. Because it is a string-keyed lookup, its failure contract is as
//! important as its success contract:
//!
//! - every documented entrypoint maps to exactly one category,
//! - the lookup is exact-match and case/whitespace sensitive, so near-miss names
//!   must resolve to `None` rather than silently picking a neighbouring rule,
//! - the derived category is round-trippable through `validate` /
//!   `validate_detailed`, and
//! - the categories it returns keep their documented zero/negative semantics.

#![cfg(test)]

extern crate alloc;

use super::*;
use soroban_sdk::symbol_short;

/// Every function name the matrix documents, with its expected category.
const MAPPED: [(&str, AmountValidationCategory); 7] = [
    ("deposit_revenue", AmountValidationCategory::RevenueDeposit),
    ("report_revenue", AmountValidationCategory::RevenueReport),
    ("set_holder_share", AmountValidationCategory::HolderShare),
    ("set_min_revenue_threshold", AmountValidationCategory::MinRevenueThreshold),
    ("set_investment_constraints", AmountValidationCategory::InvestmentMinStake),
    ("simulate_distribution", AmountValidationCategory::Simulation),
    ("set_max_total_supply_shares", AmountValidationCategory::MaxTotalSupplyShares),
];

#[test]
fn documented_entrypoints_map_to_their_category() {
    for (fn_name, expected) in MAPPED {
        assert_eq!(
            AmountValidationMatrix::category_for_function(fn_name),
            Some(expected),
            "unexpected mapping for {fn_name}"
        );
    }
}

#[test]
fn unknown_and_malformed_names_resolve_to_none() {
    let near_misses = [
        "",
        " ",
        "deposit_revenue ",
        " deposit_revenue",
        "\tdeposit_revenue",
        "DEPOSIT_REVENUE",
        "Deposit_Revenue",
        "deposit_revenue_extra",
        "deposit_revenues",
        "deposit",
        "revenue",
        "rev",
        "set_holder_share_bps",
        "setMaxTotalSupplyShares",
        "simulate_distribution()",
        "0",
    ];

    for candidate in near_misses {
        assert_eq!(
            AmountValidationMatrix::category_for_function(candidate),
            None,
            "{candidate:?} should not resolve to a category"
        );
    }
}

#[test]
fn lookup_is_deterministic_across_repeated_calls() {
    for (fn_name, expected) in MAPPED {
        for _ in 0..4 {
            assert_eq!(AmountValidationMatrix::category_for_function(fn_name), Some(expected));
        }
        assert_eq!(AmountValidationMatrix::category_for_function("nope"), None);
        assert_eq!(AmountValidationMatrix::category_for_function("nope"), None);
    }
}

#[test]
fn mapped_categories_are_unambiguous() {
    let mut seen: alloc::vec::Vec<AmountValidationCategory> = alloc::vec::Vec::new();
    for (fn_name, category) in MAPPED {
        assert!(
            !seen.contains(&category),
            "{fn_name} reuses category {category:?}; the lookup would be ambiguous for callers"
        );
        seen.push(category);
    }
    assert_eq!(seen.len(), MAPPED.len());
}

#[test]
fn every_mapped_category_accepts_a_positive_amount() {
    for (fn_name, category) in MAPPED {
        assert!(
            AmountValidationMatrix::validate(1, category).is_ok(),
            "category derived from {fn_name} must accept a positive amount"
        );
    }
}

#[test]
fn negative_amounts_are_rejected_except_for_simulation() {
    for (fn_name, category) in MAPPED {
        let negative = AmountValidationMatrix::validate(-1, category);
        if category == AmountValidationCategory::Simulation {
            assert!(
                negative.is_ok(),
                "simulation (derived from {fn_name}) is documented as allowing negatives"
            );
        } else {
            assert!(
                negative.is_err(),
                "category derived from {fn_name} must reject a negative amount"
            );
        }
    }
}

#[test]
fn zero_amount_semantics_are_preserved_for_derived_categories() {
    for (fn_name, category) in MAPPED {
        let zero = AmountValidationMatrix::validate(0, category);
        match category {
            // Revenue deposits and snapshot references are strictly positive.
            AmountValidationCategory::RevenueDeposit => {
                assert!(zero.is_err(), "deposit_revenue must reject zero ({fn_name})");
            }
            _ => {
                assert!(zero.is_ok(), "category derived from {fn_name} must accept zero");
            }
        }
    }
}

#[test]
fn simulation_category_accepts_extreme_values() {
    let category = AmountValidationMatrix::category_for_function("simulate_distribution").unwrap();

    assert!(AmountValidationMatrix::validate(0, category).is_ok());
    assert!(AmountValidationMatrix::validate(i128::MIN, category).is_ok());
    assert!(AmountValidationMatrix::validate(i128::MAX, category).is_ok());
}

#[test]
fn derived_category_round_trips_through_validate_detailed() {
    for (fn_name, category) in MAPPED {
        let derived = AmountValidationMatrix::category_for_function(fn_name).unwrap();
        assert_eq!(derived, category);

        let valid = AmountValidationMatrix::validate_detailed(10, derived);
        assert!(valid.is_valid);
        assert_eq!(valid.category, category);
        assert_eq!(valid.amount, 10);
        assert_eq!(valid.error_code, None);
        assert_eq!(valid.reason, symbol_short!("valid"));

        if category != AmountValidationCategory::Simulation {
            let invalid = AmountValidationMatrix::validate_detailed(-10, derived);
            assert!(!invalid.is_valid, "negative amount must fail for {fn_name}");
            assert!(invalid.error_code.is_some());
            assert_ne!(invalid.reason, symbol_short!("valid"));
        }
    }
}

#[test]
fn validate_batch_pins_the_first_failing_index_for_derived_categories() {
    let deposit = AmountValidationMatrix::category_for_function("deposit_revenue").unwrap();
    let report = AmountValidationMatrix::category_for_function("report_revenue").unwrap();

    assert_eq!(AmountValidationMatrix::validate_batch(&[5, 10, 15], deposit), None);
    assert_eq!(AmountValidationMatrix::validate_batch(&[5, 0, 15], deposit), Some(1));
    assert_eq!(AmountValidationMatrix::validate_batch(&[-5, -1, -9], report), Some(0));
    assert_eq!(AmountValidationMatrix::validate_batch(&[], deposit), None);
}
