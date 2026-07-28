include!("tools/storage_layout_schema.rs");

use std::env;
use std::path::PathBuf;

fn main() {
    let repo_root =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must exist"));
    verify_registry_matches_source(&repo_root)
        .expect("storage layout registry must match source enums");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must exist"));
    let generated_path = out_dir.join("STORAGE_LAYOUT.json");
    std::fs::write(&generated_path, render_storage_layout_json())
        .expect("failed to write generated storage layout json");

    println!("cargo:rerun-if-changed=tools/storage_layout_schema.rs");
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=src/revenue_deposit_contract.rs");
    println!("cargo:rerun-if-changed=src/vesting.rs");
}
