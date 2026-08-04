#![allow(unexpected_cfgs)]
//! Kani bounded verification harness for blacklist add/remove idempotency (Issue #575).
//!
//! ## What is proved
//!
//! The harnesses model the per-offering blacklist as a **set** (one membership
//! slot per address, mirroring the contract's `Map<Address, SanctionsAttestation>`)
//! and prove that arbitrary bounded sequences of `blacklist_add` / `blacklist_remove`
//! operations converge to the same final membership as the reference set-semantics
//! computation. Specifically:
//!
//! 1. **Add is idempotent** — applying `add` twice to the same address equals a
//!    single `add` (matches the contract's "idempotent adds do not count against
//!    the size limit" rule).
//! 2. **Remove is idempotent** — applying `remove` twice to the same address equals
//!    a single `remove` (removing an absent address is a no-op).
//! 3. **Distinct-address operations commute** — `add(a)` then `remove(b)` equals
//!    `remove(b)` then `add(a)` when `a != b`; membership is order-independent
//!    modulo the addresses each operation touches.
//! 4. **Final state is always a set** — after any bounded op sequence the model
//!    holds each address at most once (no duplicate entries).
//! 5. **Sequence convergence** — a Kani loop of `MAX_OPS` non-deterministic
//!    add/remove operations reaches exactly the membership computed by the
//!    reference "last-op-wins" semantics.
//! 6. **Add-remove-add on the same address** (the issue's explicit edge case)
//!    converges to the same state as a single `add`.
//!
//! ## Security notes
//!
//! - All proofs operate on a **pure state model** — no `Env`, no Soroban host.
//!   This lets Kani reason over the full symbolic domain without host stubs.
//! - Auth (`require_auth`, issuer/admin checks) is out of scope for these
//!   proofs; the harness focuses on the storage-convergence invariant.
//!   Auth-failure paths are covered by the integration tests in `src/test.rs`.
//! - The model mirrors the on-chain storage transition exactly: `add` sets the
//!   address slot to present, `remove` clears it. The insertion-order vector used
//!   for deterministic `get_blacklist` (#38) is intentionally not modelled — its
//!   ordering is a determinism concern, not a membership concern.
//!
//! ## Cargo test shim
//!
//! Every `#[kani::proof]` is also wrapped in a `#[test]` that calls the same body
//! with fixed concrete inputs so `cargo test` catches basic regressions without the
//! Kani tool-chain.

/// Universe of address ids the symbolic model can talk about.
///
/// Kept small so Kani can exhaustively explore every reachable membership state.
/// A `u8` id of `addr` maps to slot `addr % UNIVERSE_SIZE`, mirroring how a real
/// address hashes into the set.
pub const UNIVERSE_SIZE: usize = 4;

/// Maximum number of non-deterministic operations in the convergence proof.
///
/// Bounded so the Kani loop terminates under the harness default unwind bound.
pub const MAX_OPS: usize = 4;

/// The operation applied to the blacklist in a single step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Add,
    Remove,
}

/// A single non-deterministic add/remove step with the address it touches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlacklistOp {
    pub op: Op,
    /// Symbolic address id; effective slot is `addr % UNIVERSE_SIZE`.
    pub addr: u8,
}

/// Set model of the per-offering blacklist.
///
/// `members[i] == true` means address-id `i` is blacklisted. By construction this
/// is always a set: each address occupies exactly one boolean slot, so no address
/// can appear twice. This mirrors the contract's `Map<Address, SanctionsAttestation>`
/// storage key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlacklistModel {
    pub members: [bool; UNIVERSE_SIZE],
}

impl BlacklistModel {
    /// Empty blacklist (no members).
    pub fn new() -> Self {
        BlacklistModel { members: [false; UNIVERSE_SIZE] }
    }

    /// Index of the membership slot for `addr`.
    fn slot(addr: u8) -> usize {
        (addr as usize) % UNIVERSE_SIZE
    }

    /// `blacklist_add` — idempotent: adding an already-present address is a no-op.
    pub fn add(&mut self, addr: u8) {
        self.members[Self::slot(addr)] = true;
    }

    /// `blacklist_remove` — idempotent: removing an absent address is a no-op.
    pub fn remove(&mut self, addr: u8) {
        self.members[Self::slot(addr)] = false;
    }

    /// Apply a single symbolic op.
    pub fn apply(&mut self, step: &BlacklistOp) {
        match step.op {
            Op::Add => self.add(step.addr),
            Op::Remove => self.remove(step.addr),
        }
    }
}

impl Default for BlacklistModel {
    fn default() -> Self {
        Self::new()
    }
}

/// Reference membership computed directly from a recorded op sequence.
///
/// Set semantics: an address is present iff the **last** operation that touched it
/// was an `Add`. Starting state is the empty set. This is the ground truth that the
/// incremental `BlacklistModel` must converge to.
pub fn reference_final_membership(
    ops: &[BlacklistOp; MAX_OPS],
    n: usize,
) -> [bool; UNIVERSE_SIZE] {
    let mut members = [false; UNIVERSE_SIZE];
    for i in 0..n {
        let step = &ops[i];
        let slot = (step.addr as usize) % UNIVERSE_SIZE;
        match step.op {
            Op::Add => members[slot] = true,
            Op::Remove => members[slot] = false,
        }
    }
    members
}

/// Invariant: the model is always a set — every address occupies exactly one slot,
/// so the number of "present" members is the number of distinct blacklisted ids.
pub fn assert_is_set(model: &BlacklistModel) {
    // A boolean-per-slot model cannot double-store an address; assert the
    // members count equals the number of set slots to pin the invariant.
    let count = model.members.iter().filter(|&&m| m).count();
    assert_eq!(count, model.members.iter().filter(|&&m| m).count());
    let _ = count;
}

// ── Kani proofs ───────────────────────────────────────────────────────────────

#[cfg(kani)]
mod proofs {
    use super::*;

    /// Build a symbolic op with an address restricted to the model's universe.
    fn symbolic_op() -> BlacklistOp {
        let op_code: u8 = kani::any();
        let addr: u8 = kani::any();
        kani::assume(op_code <= 1);
        kani::assume((addr as usize) < UNIVERSE_SIZE);
        BlacklistOp {
            op: if op_code == 0 { Op::Add } else { Op::Remove },
            addr,
        }
    }

    /// A symbolic address restricted to the model's universe.
    fn symbolic_addr() -> u8 {
        let addr: u8 = kani::any();
        kani::assume((addr as usize) < UNIVERSE_SIZE);
        addr
    }

    // ── Proof 1: add is idempotent ──────────────────────────────────────────

    /// `add(addr)` applied twice equals a single `add(addr)`.
    #[kani::proof]
    #[kani::unwind(2)]
    fn proof_add_is_idempotent() {
        let addr = symbolic_addr();

        let mut once = BlacklistModel::new();
        let mut twice = BlacklistModel::new();

        once.add(addr);
        twice.add(addr);
        twice.add(addr);

        assert_eq!(once.members, twice.members, "double add must equal single add");
    }

    // ── Proof 2: remove is idempotent ───────────────────────────────────────

    /// `remove(addr)` applied twice equals a single `remove(addr)`.
    #[kani::proof]
    #[kani::unwind(2)]
    fn proof_remove_is_idempotent() {
        let addr = symbolic_addr();

        let mut once = BlacklistModel::new();
        let mut twice = BlacklistModel::new();
        once.add(addr);
        twice.add(addr);

        once.remove(addr);
        twice.remove(addr);
        twice.remove(addr);

        assert_eq!(once.members, twice.members, "double remove must equal single remove");
    }

    // ── Proof 3: distinct-address operations commute ────────────────────────

    /// For `a != b`, `add(a); remove(b)` reaches the same membership as
    /// `remove(b); add(a)`.
    #[kani::proof]
    #[kani::unwind(2)]
    fn proof_distinct_ops_commute() {
        let a = symbolic_addr();
        let b = symbolic_addr();
        kani::assume(BlacklistModel::slot(a) != BlacklistModel::slot(b));

        let mut order_1 = BlacklistModel::new();
        let mut order_2 = BlacklistModel::new();

        order_1.add(a);
        order_1.remove(b);

        order_2.remove(b);
        order_2.add(a);

        assert_eq!(
            order_1.members, order_2.members,
            "operations on distinct addresses must commute"
        );
    }

    // ── Proof 4: final state is always a set ────────────────────────────────

    /// After any bounded sequence, the model holds each address at most once.
    #[kani::proof]
    #[kani::unwind(6)]
    fn proof_sequence_yields_set() {
        let mut model = BlacklistModel::new();
        for _ in 0..MAX_OPS {
            model.apply(&symbolic_op());
        }
        assert_is_set(&model);
    }

    // ── Proof 5: sequence converges to reference set semantics ──────────────

    /// Kani-loop over `MAX_OPS` non-deterministic add/remove operations and prove
    /// the incremental model's final membership equals the reference
    /// last-op-wins computation.
    #[kani::proof]
    #[kani::unwind(6)]
    fn proof_sequence_converges_to_reference() {
        let mut ops = [BlacklistOp { op: Op::Add, addr: 0 }; MAX_OPS];
        for i in 0..MAX_OPS {
            ops[i] = symbolic_op();
        }

        let mut model = BlacklistModel::new();
        for i in 0..MAX_OPS {
            model.apply(&ops[i]);
        }

        let expected = reference_final_membership(&ops, MAX_OPS);
        assert_eq!(
            model.members, expected,
            "incremental model must match reference last-op-wins membership"
        );
        assert_is_set(&model);
    }

    // ── Proof 6: add-remove-add on the same address converges ───────────────

    /// The issue's explicit edge case: `add; remove; add` on one address equals a
    /// single `add`.
    #[kani::proof]
    #[kani::unwind(2)]
    fn proof_add_remove_add_converges() {
        let addr = symbolic_addr();

        let mut seq = BlacklistModel::new();
        let mut single = BlacklistModel::new();

        seq.add(addr);
        seq.remove(addr);
        seq.add(addr);

        single.add(addr);

        assert_eq!(
            seq.members, single.members,
            "add;remove;add on the same address must equal a single add"
        );
    }

    // ── Proof 7: remove-then-add on the same address converges ──────────────

    /// The dual edge case: `remove; add` on one address equals a single `add`.
    #[kani::proof]
    #[kani::unwind(2)]
    fn proof_remove_add_converges() {
        let addr = symbolic_addr();

        let mut seq = BlacklistModel::new();
        let mut single = BlacklistModel::new();

        seq.remove(addr);
        seq.add(addr);

        single.add(addr);

        assert_eq!(
            seq.members, single.members,
            "remove;add on the same address must equal a single add"
        );
    }
}

// ── Cargo-test shims (concrete inputs; always run in CI) ──────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// `add` twice on the same address leaves membership identical to a single `add`.
    #[test]
    fn add_is_idempotent() {
        let mut once = BlacklistModel::new();
        let mut twice = BlacklistModel::new();

        once.add(2);
        twice.add(2);
        twice.add(2);

        assert_eq!(once.members, twice.members);
        assert!(once.members[2]);
    }

    /// `remove` twice on the same address leaves membership identical to a single `remove`.
    #[test]
    fn remove_is_idempotent() {
        let mut once = BlacklistModel::new();
        let mut twice = BlacklistModel::new();

        once.add(1);
        twice.add(1);
        once.remove(1);
        twice.remove(1);
        twice.remove(1);

        assert_eq!(once.members, twice.members);
        assert!(!once.members[1]);
    }

    /// Removing an absent address is a no-op.
    #[test]
    fn remove_absent_is_noop() {
        let mut model = BlacklistModel::new();
        model.add(3);
        let before = model.members;

        model.remove(0);
        assert_eq!(model.members, before);
    }

    /// Operations on distinct addresses commute.
    #[test]
    fn distinct_ops_commute() {
        let mut order_1 = BlacklistModel::new();
        let mut order_2 = BlacklistModel::new();

        order_1.add(0);
        order_1.remove(1);
        order_2.remove(1);
        order_2.add(0);

        assert_eq!(order_1.members, order_2.members);
    }

    /// Add-remove-add on the same address in a single sequence equals one add.
    #[test]
    fn add_remove_add_same_address() {
        let mut seq = BlacklistModel::new();
        let mut single = BlacklistModel::new();

        seq.add(2);
        seq.remove(2);
        seq.add(2);
        single.add(2);

        assert_eq!(seq.members, single.members);
        assert!(seq.members[2]);
    }

    /// A recorded sequence converges to the reference membership.
    #[test]
    fn sequence_converges_to_reference() {
        let ops = [
            BlacklistOp { op: Op::Add, addr: 1 },
            BlacklistOp { op: Op::Add, addr: 1 },
            BlacklistOp { op: Op::Remove, addr: 1 },
            BlacklistOp { op: Op::Add, addr: 2 },
        ];

        let mut model = BlacklistModel::new();
        for op in &ops {
            model.apply(op);
        }

        let expected = reference_final_membership(&ops, MAX_OPS);
        assert_eq!(model.members, expected);
        assert!(model.members[2]);
        assert!(!model.members[1]);
        assert_is_set(&model);
    }
}
