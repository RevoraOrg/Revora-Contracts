"""Regenerate tools/storage_layout_schema.rs + docs/STORAGE_LAYOUT.json.

Mirrors the Rust extractor in tools/storage_layout_schema.rs (extract_enum_variants)
so the generated registry matches the source enums exactly. Annotations (value_type,
ownership) are harvested from the current registry and, with DataKey3 renamed to
DataKey2, from the 8c7b052 registry.
"""
import json
import re
import subprocess

# Must match STORAGE_LAYOUT_VERSION in src/lib.rs (asserted by
# tests/storage_layout_json.rs::generated_storage_layout_version_matches_contract_constant).
LAYOUT_VERSION = 5
SCHEMA_VERSION = 1

OLD_REGISTRY_REF = "8c7b052"
# Registry snapshot that already carried the full DataKey3 annotation table.
OLD_REGISTRY_REF_DATAKEY3 = "60c8bfa"


def extract_variants(source, enum_name):
    """Faithful port of extract_enum_variants from tools/storage_layout_schema.rs."""
    variants, in_enum = [], False
    for raw_line in source.split("\n"):
        line = raw_line.strip()
        if not in_enum:
            if (line.startswith("pub enum ") or line.startswith("pub(crate) enum ")) and line.endswith("{"):
                if line[:-1].split()[-1] == enum_name:
                    in_enum = True
            continue
        if line == "}":
            break
        if not line or line.startswith("///") or line.startswith("//") or line.startswith("#["):
            continue
        if line.endswith(","):
            variants.append(line[:-1].strip())
    return variants


def harvest_annotations():
    ann = {}
    cur = open("tools/storage_layout_schema.rs").read()
    for m in re.finditer(r'\("([A-Za-z0-9_]+::[^"]+)",\s*"([^"]*)",\s*"([^"]*)"\)', cur):
        ann[m.group(1)] = (m.group(2), m.group(3))
    old = subprocess.run(
        ["git", "show", f"{OLD_REGISTRY_REF}:tools/storage_layout_schema.rs"],
        capture_output=True, text=True,
    ).stdout
    for m in re.finditer(r'\("([A-Za-z0-9_]+::[^"]+)",\s*"([^"]*)",\s*"([^"]*)"\)', old):
        k = m.group(1).replace("DataKey3::", "DataKey2::")
        ann.setdefault(k, (m.group(2), m.group(3)))
    old3 = subprocess.run(
        ["git", "show", f"{OLD_REGISTRY_REF_DATAKEY3}:tools/storage_layout_schema.rs"],
        capture_output=True, text=True,
    ).stdout
    for m in re.finditer(r'\("([A-Za-z0-9_]+::[^"]+)",\s*"([^"]*)",\s*"([^"]*)"\)', old3):
        ann.setdefault(m.group(1), (m.group(2), m.group(3)))
    return ann


GROUPS = [
    ("revora_revenue_share", [
        ("src/lib.rs", ["DeferredDataKey", "WindowDataKey", "MetaDataKey", "DataKey", "DataKey2", "DataKey3", "FaucetDataKey", "MigrationDataKey"]),
    ]),
    ("revenue_deposit_contract", [
        ("src/revenue_deposit_contract.rs", ["DataKey"]),
    ]),
    ("vesting_contract", [
        ("src/vesting.rs", ["VestingKey"]),
    ]),
]

HEADER = r'''use std::collections::BTreeSet;
use std::format;
use std::fs;
use std::path::Path;

pub const STORAGE_LAYOUT_SCHEMA_VERSION: u32 = 1;
pub const STORAGE_LAYOUT_VERSION: u32 = %(layout_version)s;

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
'''

TAIL = r'''pub fn all_storage_layout_entries() -> Vec<StorageLayoutEntry> {
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
        ("src/lib.rs", "DataKey3"),
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
'''


def esc(s):
    return s.replace("\\", "\\\\").replace('"', '\\"')


def build_table(targets, ann):
    lines = []
    for path, enums in targets:
        src = open(path).read()
        for e in enums:
            vs = extract_variants(src, e)
            lines.append("    // -- %s --" % e)
            for v in vs:
                k = "%s::%s" % (e, v)
                if k not in ann:
                    raise SystemExit("missing annotation for %s" % k)
                vt, ow = ann[k]
                lines.append('    ("%s", "%s", "%s"),' % (esc(k), esc(vt), esc(ow)))
    return "\n".join(lines)


def render_json(entries):
    out = ['{']
    out.append('  "schema_version": %d,' % SCHEMA_VERSION)
    out.append('  "layout_version": %d,' % LAYOUT_VERSION)
    out.append('  "entries": [')
    for i, e in enumerate(entries):
        out.append("    {")
        out.append('      "module": %s,' % json.dumps(e["module"]))
        out.append('      "key": %s,' % json.dumps(e["key"]))
        out.append('      "value_type": %s,' % json.dumps(e["value_type"]))
        out.append('      "ownership": %s,' % json.dumps(e["ownership"]))
        out.append('      "version": %d' % e["version"])
        out.append("    }" + ("," if i + 1 != len(entries) else ""))
    out.append("  ]")
    out.append("}")
    return "\n".join(out) + "\n"


def main():
    ann = harvest_annotations()
    tables, all_entries = {}, []
    for module, targets in GROUPS:
        table = build_table(targets, ann)
        tables[module] = table
        for line in table.split("\n"):
            m = re.match(r'    \("([^"]+)", "([^"]*)", "([^"]*)"\),', line)
            if m:
                all_entries.append({
                    "module": module,
                    "key": m.group(1).replace("\\\\", "\\").replace('\\"', '"'),
                    "value_type": m.group(2),
                    "ownership": m.group(3),
                    "version": LAYOUT_VERSION,
                })
    all_entries.sort(key=lambda e: e["key"])

    core = tables["revora_revenue_share"]
    rev = tables["revenue_deposit_contract"]
    vest = tables["vesting_contract"]

    content = (
        HEADER % {"layout_version": LAYOUT_VERSION}
        + '\nconst CORE_LAYOUT: &[StorageLayoutEntry] = storage_layout_entries!("revora_revenue_share", [\n'
        + core + "\n]);\n"
        + '\nconst REVENUE_DEPOSIT_LAYOUT: &[StorageLayoutEntry] = storage_layout_entries!("revenue_deposit_contract", [\n'
        + rev + "\n]);\n"
        + '\nconst VESTING_LAYOUT: &[StorageLayoutEntry] = storage_layout_entries!("vesting_contract", [\n'
        + vest + "\n]);\n"
        + "\n" + TAIL
    )
    open("tools/storage_layout_schema.rs", "w").write(content)
    print("registry written: %d lines, %d entries" % (content.count("\n"), len(all_entries)))

    open("docs/STORAGE_LAYOUT.json", "w").write(render_json(all_entries))
    print("docs/STORAGE_LAYOUT.json written")


if __name__ == "__main__":
    main()
