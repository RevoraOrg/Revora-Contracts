mod storage_layout_schema {
    include!("../tools/storage_layout_schema.rs");
}

#[test]
fn generated_storage_layout_version_matches_contract_constant() {
    assert_eq!(
        storage_layout_schema::STORAGE_LAYOUT_VERSION,
        revora_contracts::STORAGE_LAYOUT_VERSION,
    );
}

#[test]
fn storage_layout_json_matches_checked_in_docs() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    storage_layout_schema::verify_registry_matches_source(repo_root)
        .expect("storage layout registry must cover every key enum variant");

    let generated = storage_layout_schema::render_storage_layout_json();
    let checked_in = std::fs::read_to_string(repo_root.join("docs/STORAGE_LAYOUT.json"))
        .expect("docs/STORAGE_LAYOUT.json must exist");

    assert_eq!(
        generated, checked_in,
        "docs/STORAGE_LAYOUT.json is out of date; regenerate it from tools/storage_layout_schema.rs",
    );
}
