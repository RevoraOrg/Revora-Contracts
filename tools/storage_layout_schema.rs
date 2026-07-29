use std::collections::BTreeSet;
use std::format;
use std::fs;
use std::path::Path;

pub const STORAGE_LAYOUT_SCHEMA_VERSION: u32 = 1;
pub const STORAGE_LAYOUT_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StorageLayoutEntry {
    pub module: &'static str,
    pub key: &'static str,
    pub value_type: &'static str,
    pub ownership: &'static str,
    pub version: u32,
}

macro_rules! storage_layout_entries {
    ($module:expr, [$(($key:expr, $value_type:expr, $ownership:expr)),+ $(,)?]) => {
        &[
            $(
                StorageLayoutEntry {
                    module: $module,
                    key: $key,
                    value_type: $value_type,
                    ownership: $ownership,
                    version: STORAGE_LAYOUT_VERSION,
                },
            )+
        ]
    };
}

const CORE_LAYOUT: &[StorageLayoutEntry] = storage_layout_entries!("revora_revenue_share", [
    ("DeferredDataKey::DeferredReports(u32)", "i128", "period"),
    ("WindowDataKey::Report(OfferingId)", "WindowConfig", "offering"),
    ("WindowDataKey::Claim(OfferingId)", "WindowConfig", "offering"),
    ("WindowDataKey::Redemption(OfferingId)", "WindowConfig", "offering"),
    ("MetaDataKey::SignerKey(Address)", "BytesN<32>", "address"),
    ("MetaDataKey::Delegate(OfferingId)", "Address", "offering"),
    ("MetaDataKey::NonceUsed(Address, u64)", "bool", "address+nonce"),
    ("MetaDataKey::RevenueApproved(OfferingId, u64)", "bool", "offering+period"),
    ("DataKey::LastPeriodId(OfferingId)", "u64", "offering"),
    ("DataKey::Blacklist(OfferingId)", "Vec<Address>", "offering"),
    ("DataKey::Whitelist(OfferingId)", "Vec<Address>", "offering"),
    ("DataKey::BlacklistOrder(OfferingId)", "Vec<Address>", "offering"),
    ("DataKey::OfferCount(TenantId)", "u32", "tenant"),
    ("DataKey::OfferItem(TenantId, u32)", "OfferingId", "tenant+index"),
    ("DataKey::ConcentrationLimit(OfferingId)", "u32", "offering"),
    ("DataKey::CurrentConcentration(OfferingId)", "u32", "offering"),
    ("DataKey::ConcentrationReportedAt(OfferingId)", "u64", "offering"),
    ("DataKey::AuditSummary(OfferingId)", "AuditSummary", "offering"),
    ("DataKey::RoundingMode(OfferingId)", "RoundingMode", "offering"),
    ("DataKey::RevenueReports(OfferingId)", "Map<u64, RevenueReport>", "offering"),
    ("DataKey::RevenueIndex(OfferingId, u64)", "i128", "offering+period"),
    ("DataKey::PeriodRevenue(OfferingId, u64)", "i128", "offering+period"),
    ("DataKey::PeriodEntry(OfferingId, u32)", "u64", "offering+index"),
    ("DataKey::PeriodCount(OfferingId)", "u32", "offering"),
    ("DataKey::AccrualIndexE18(OfferingId)", "i128", "offering"),
    ("DataKey::HolderShare(OfferingId, Address)", "u32", "offering+holder"),
    ("DataKey::LastClaimedAccrualIndex(OfferingId, Address)", "i128", "offering+holder"),
    ("DataKey::HolderShareTotal(OfferingId)", "u32", "offering"),
    ("DataKey::LastClaimedIdx(OfferingId, Address)", "u32", "offering+holder"),
    ("DataKey::PaymentToken(OfferingId)", "Address", "offering"),
    ("DataKey::ClaimDelaySecs(OfferingId)", "u64", "offering"),
    ("DataKey::PeriodDepositTime(OfferingId, u64)", "u64", "offering+period"),
    ("DataKey::Admin", "Address", "contract"),
    ("DataKey::Frozen", "bool", "contract"),
    ("DataKey::PendingAdmin", "PendingAdminRotation", "contract"),
    ("DataKey::SnapshotConfig(OfferingId)", "bool", "offering"),
    ("DataKey::LastSnapshotRef(OfferingId)", "u64", "offering"),
    ("DataKey::SnapshotEntry(OfferingId, u64)", "SnapshotEntry", "offering+snapshot"),
    ("DataKey::SnapshotHolder(OfferingId, u64, u32)", "HolderSnapshotEntry", "offering+snapshot+index"),
    ("DataKey::SnapshotHolderCount(OfferingId, u64)", "u32", "offering+snapshot"),
    ("DataKey::PendingIssuerTransfer(OfferingId)", "PendingTransfer", "offering"),
    ("DataKey::OfferingIssuer(OfferingId)", "Address", "offering"),
    ("DataKey::TestnetMode", "bool", "contract"),
    ("DataKey::Safety", "Address", "contract"),
    ("DataKey::Paused", "PauseState", "contract"),
    ("DataKey::EventOnlyMode", "bool", "contract"),
    ("DataKey::DeployedVersion", "u32", "contract"),
    ("DataKey::StorageLayoutVersion", "u32", "contract"),
    ("DataKey::PlatformFeeBps", "u32", "contract"),
    ("DataKey::OfferingFeeBps(OfferingId, Address)", "u32", "offering+asset"),
    ("DataKey::PlatformFeePerAsset(Address)", "u32", "asset"),
    ("DataKey::SnapshotFinalizationRequired", "bool", "contract"),
    ("DataKey::LastSnapshotCommitRef(OfferingId)", "u64", "offering"),
    ("DataKey2::SnapshotFinalized(OfferingId, u64)", "bool", "offering+snapshot"),
    ("DataKey2::SupplyCap(OfferingId)", "i128", "offering"),
    ("DataKey2::DepositedRevenue(OfferingId)", "i128", "offering"),
    ("DataKey2::InvestmentConstraints(OfferingId)", "InvestmentConstraints", "offering"),
    ("DataKey2::MinRevenueThreshold(OfferingId)", "i128", "offering"),
    ("DataKey2::LastReportedPeriodId(OfferingId)", "u64", "offering"),
    ("DataKey2::LastDepositedPeriodId(OfferingId)", "u64", "offering"),
    ("DataKey2::PaymentTokenDecimals(OfferingId)", "u32", "offering"),
    ("DataKey2::FrozenOffering(OfferingId)", "bool", "offering"),
    ("DataKey2::IssuerCount", "u32", "contract"),
    ("DataKey2::IssuerItem(u32)", "Address", "issuer+index"),
    ("DataKey2::IssuerRegistered(Address)", "bool", "issuer"),
    ("DataKey2::NamespaceCount(Address)", "u32", "issuer"),
    ("DataKey2::NamespaceItem(Address, u32)", "Symbol", "issuer+index"),
    ("DataKey2::NamespaceRegistered(Address, Symbol)", "bool", "issuer+namespace"),
    ("DataKey2::StressDataEntry(Address, u32)", "Bytes", "admin+index"),
    ("DataKey2::StressDataCount(Address)", "u32", "admin"),
    ("DataKey2::HolderJurisdiction(OfferingId, Address)", "Symbol", "offering+holder"),
    ("DataKey2::AllowedJurisdictions(OfferingId)", "Vec<Symbol>", "offering"),
    ("DataKey2::GlobalAccPerShareE18(OfferingId)", "i128", "offering"),
    ("DataKey2::AccPerShareAtIndex(OfferingId, u32)", "i128", "offering+index"),
    ("DataKey2::HolderAccrualState(OfferingId, Address)", "HolderAccrualState", "offering+holder"),
    ("DataKey2::HolderShareSchedule(OfferingId, Address)", "Vec<ShareCheckpoint>", "offering+holder"),
    ("DataKey2::ContractFlags", "u32", "contract"),
    ("DataKey2::OfferingRecord(OfferingId)", "Offering", "offering"),
    ("DataKey2::BlacklistSizeLimit(OfferingId)", "u32", "offering"),
    ("DataKey2::ClosedPeriod(OfferingId, u64)", "bool", "offering+period"),
    ("DataKey2::DisclosureMeta(OfferingId)", "DisclosureMeta", "offering"),
    ("DataKey2::FaucetLastRequest(Address)", "u64", "address"),
    ("DataKey2::DualSigEnabled(OfferingId)", "bool", "offering"),
    ("DataKey2::AdminRotationLog(u64)", "AdminRotationEntry", "contract"),
    ("DataKey2::AdminRotationCount", "u64", "contract"),
    ("DataKey2::RedemptionRequest(OfferingId, Address)", "PendingRedemption", "offering+holder"),
    ("DataKey2::RedemptionFeeConfig(OfferingId)", "RedemptionFeeConfig", "offering"),
    ("DataKey2::AdminRotationDelay", "u64", "contract"),
    ("DataKey2::MultisigOwners", "Vec<Address>", "contract"),
    ("DataKey2::MultisigThreshold", "u32", "contract"),
    ("DataKey2::MultisigProposalCount", "u32", "contract"),
    ("DataKey2::MultisigProposalDuration", "u64", "contract"),
    ("DataKey2::MultisigProposal(u32)", "GovernanceProposal", "proposal"),
    ("DataKey2::VoterWeight(Address)", "u32", "address"),
    ("DataKey2::MultisigQuorumBps", "u32", "contract"),
    ("MigrationDataKey::LastMigrationCompletedAt(Address)", "u32", "issuer")
]);

const REVENUE_DEPOSIT_LAYOUT: &[StorageLayoutEntry] = storage_layout_entries!("revenue_deposit_contract", [
    ("DataKey::Admin", "Address", "contract"),
    ("DataKey::Token", "Address", "contract"),
    ("DataKey::AuthorizedOfferings", "Vec<Address>", "contract"),
    ("DataKey::PeriodCounter", "u32", "contract"),
    ("DataKey::PeriodIds", "Vec<u32>", "contract"),
    ("DataKey::Period(u32)", "Period", "period"),
    ("DataKey::Beneficiaries(u32)", "Vec<Address>", "period"),
    ("DataKey::Claimed(u32, Address)", "bool", "period+holder")
]);

const VESTING_LAYOUT: &[StorageLayoutEntry] = storage_layout_entries!("vesting_contract", [
    ("VestingKey::Schedule(Address)", "VestingSchedule", "beneficiary"),
    ("VestingKey::Claimed(Address)", "i128", "beneficiary"),
    ("VestingKey::OfferingScheduleCount(VestingOfferingId)", "u32", "offering"),
    ("VestingKey::OfferingScheduleItem(VestingOfferingId, u32)", "Address", "offering+index"),
    ("VestingKey::Acceleration(Address, Symbol)", "bool", "beneficiary+trigger")
]);

pub fn all_storage_layout_entries() -> Vec<StorageLayoutEntry> {
    let mut entries = Vec::new();
    entries.extend_from_slice(CORE_LAYOUT);
    entries.extend_from_slice(REVENUE_DEPOSIT_LAYOUT);
    entries.extend_from_slice(VESTING_LAYOUT);
    entries.sort_by(|left, right| left.key.cmp(right.key));
    entries
}

pub fn render_storage_layout_json() -> String {
    let entries = all_storage_layout_entries();
    let mut json = String::new();
    json.push_str("{\n");
    json.push_str(&format!(
        "  \"schema_version\": {},\n  \"layout_version\": {},\n  \"entries\": [\n",
        STORAGE_LAYOUT_SCHEMA_VERSION, STORAGE_LAYOUT_VERSION
    ));

    for (index, entry) in entries.iter().enumerate() {
        json.push_str("    {\n");
        json.push_str(&format!("      \"module\": \"{}\",\n", escape_json(entry.module)));
        json.push_str(&format!("      \"key\": \"{}\",\n", escape_json(entry.key)));
        json.push_str(&format!(
            "      \"value_type\": \"{}\",\n",
            escape_json(entry.value_type)
        ));
        json.push_str(&format!(
            "      \"ownership\": \"{}\",\n",
            escape_json(entry.ownership)
        ));
        json.push_str(&format!("      \"version\": {}\n", entry.version));
        json.push_str("    }");
        if index + 1 != entries.len() {
            json.push(',');
        }
        json.push('\n');
    }

    json.push_str("  ]\n}\n");
    json
}

pub fn verify_registry_matches_source(repo_root: &Path) -> Result<(), String> {
    let expected: BTreeSet<String> = all_storage_layout_entries()
        .into_iter()
        .map(|entry| entry.key.to_string())
        .collect();

    let actual = collect_source_keys(repo_root)?;
    if actual != expected {
        let missing: Vec<_> = actual.difference(&expected).cloned().collect();
        let stale: Vec<_> = expected.difference(&actual).cloned().collect();
        let mut message = String::from("storage layout registry drift detected");
        if !missing.is_empty() {
            message.push_str(&format!("\nmissing registrations: {}", missing.join(", ")));
        }
        if !stale.is_empty() {
            message.push_str(&format!("\nstale registrations: {}", stale.join(", ")));
        }
        return Err(message);
    }
    Ok(())
}

fn collect_source_keys(repo_root: &Path) -> Result<BTreeSet<String>, String> {
    let targets = [
        ("src/lib.rs", "DeferredDataKey"),
        ("src/lib.rs", "WindowDataKey"),
        ("src/lib.rs", "MetaDataKey"),
        ("src/lib.rs", "DataKey"),
        ("src/lib.rs", "DataKey2"),
        ("src/lib.rs", "MigrationDataKey"),
        ("src/revenue_deposit_contract.rs", "DataKey"),
        ("src/vesting.rs", "VestingKey"),
    ];

    let mut keys = BTreeSet::new();
    for (path, enum_name) in targets {
        let file = repo_root.join(path);
        let contents = fs::read_to_string(&file)
            .map_err(|error| format!("failed to read {}: {}", file.display(), error))?;
        for variant in extract_enum_variants(&contents, enum_name) {
            keys.insert(format!("{}::{}", enum_name, variant));
        }
    }

    Ok(keys)
}

fn extract_enum_variants(source: &str, enum_name: &str) -> Vec<String> {
    let mut variants = Vec::new();
    let mut in_enum = false;

    for raw_line in source.lines() {
        let line = raw_line.trim();
        if !in_enum {
            if let Some(candidate) = enum_name_from_declaration(line) {
                if candidate == enum_name {
                    in_enum = true;
                }
            }
            continue;
        }

        if line == "}" {
            break;
        }
        if line.is_empty()
            || line.starts_with("///")
            || line.starts_with("//")
            || line.starts_with("#[")
        {
            continue;
        }

        if let Some(stripped) = line.strip_suffix(',') {
            variants.push(stripped.trim().to_string());
        }
    }

    variants
}

fn enum_name_from_declaration(line: &str) -> Option<&str> {
    if !(line.starts_with("pub enum ") || line.starts_with("pub(crate) enum ")) {
        return None;
    }

    let before_brace = line.strip_suffix('{')?.trim();
    before_brace.split_whitespace().last()
}

fn escape_json(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}
