const fs = require("fs");

const libPath = "src/lib.rs";
let lib = fs.readFileSync(libPath, "utf8");

// 1. Inject Structs near the top
lib = lib.replace(/(use soroban_sdk::[^;]+;)/, match => {
    return match + `\n\n#[soroban_sdk::contracterror]\n#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]\n#[repr(u32)]\npub enum DistributionError { DistributionDeferred = 456 }\n\n#[soroban_sdk::contracttype]\npub enum DeferredDataKey { DeferredReports(u32) }\n`;
});

// 2. Patch report_revenue
lib = lib.replace(/(pub\s+fn\s+report_revenue\s*\(\s*[^)]+)\)\s*\{/, (match, p1) => {
    return p1 + `, defer_until_close: bool) {\n        if defer_until_close {\n            env.storage().persistent().set(&DeferredDataKey::DeferredReports(period_id), &amount);\n            env.events().publish((soroban_sdk::symbol_short!("def_report"), period_id), amount);\n            return;\n        }\n`;
});

// 3. Patch claim
lib = lib.replace(/(pub\s+fn\s+claim\s*\(\s*[^)]+\)\s*\{)/, match => {
    return match + `\n        if env.storage().persistent().has(&DeferredDataKey::DeferredReports(period_id)) {\n            soroban_sdk::panic_with_error!(&env, DistributionError::DistributionDeferred);\n        }\n`;
});

// 4. Inject new entrypoints right before the last closing bracket of the file
lib = lib.replace(/\}\s*$/, `
    pub fn replace_deferred(env: soroban_sdk::Env, period_id: u32, new_amount: i128) {
        if env.storage().persistent().has(&DeferredDataKey::DeferredReports(period_id)) {
            env.storage().persistent().set(&DeferredDataKey::DeferredReports(period_id), &new_amount);
        }
    }

    pub fn close_period(env: soroban_sdk::Env, period_id: u32) {
        let deferred_key = DeferredDataKey::DeferredReports(period_id);
        if let Some(amount) = env.storage().persistent().get::<_, i128>(&deferred_key) {
            env.storage().persistent().remove(&deferred_key);
            env.events().publish((soroban_sdk::symbol_short!("def_flush"), period_id), amount);
        }
    }
}
`);

fs.writeFileSync(libPath, lib);

// 5. Build Tests dynamically
const match = lib.match(/impl\s+([A-Za-z0-9_]+)\s*\{/);
const contractName = match ? match[1] : "Contract";
const clientName = contractName + "Client";

const testPath = "src/test_close_period.rs";
const testCode = `
#![cfg(test)]
use super::*;
use soroban_sdk::{Env, testutils::Events};

#[test]
#[should_panic(expected = "Error(Contract, #456)")]
fn test_claim_on_deferred_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, ${contractName});
    let client = ${clientName}::new(&env, &contract_id);

    client.report_revenue(&2, &5000, &true);
    client.claim(&2);
}
`;
fs.writeFileSync(testPath, testCode);

if (!lib.includes("mod test_close_period;")) {
    fs.appendFileSync(libPath, "\n#[cfg(test)]\nmod test_close_period;\n");
}

// 6. Write Docs
const docPath = "docs/DEFERRED_DISTRIBUTIONS.md";
if (!fs.existsSync("docs")) { fs.mkdirSync("docs", { recursive: true }); }
fs.writeFileSync(docPath, "# Deferred Distributions\n\nAdds a `defer_until_close` flag to revenue reports. \n\n### Lifecycle\n1. **Queueing:** Deferred reports are stored in the `DeferredReports` mapping keyed by `period_id`.\n2. **Security Barrier:** Any `claim` attempt against a period still in the deferred mapping will immediately panic with `DistributionDeferred`.\n3. **Atomic Flush:** Calling `close_period` removes the block.\n");

console.log("? Clean Auto-Patcher completed successfully!");
