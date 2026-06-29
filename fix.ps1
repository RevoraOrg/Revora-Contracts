$content = Get-Content -Path "src/lib.rs" -Raw

$content = $content -replace "Map, String, Symbol", "Map, Symbol"
$content = $content -replace "EVENT_REV_INIA_V1", "EVENT_REV_INIA_V2"
$content = $content -replace "EVENT_REV_REP_V1", "EVENT_REV_REP_V2"
$content = $content -replace "EVENT_REV_REPA_V1", "EVENT_REV_REPA_V2"
$content = $content -replace 'const EVENT_INDEXED_V2: Symbol = symbol_short!\("ev_idx2"\);', "const EVENT_INDEXED_V2: Symbol = symbol_short!`"ev_idx2`";`r`nconst EVENT_DECIMAL_SET: Symbol = symbol_short!`"dec_set`";"
$content = $content -replace '(?m)^#\[cfg_attr\(test, derive\(proptest::prelude::Arbitrary\)\)\]\r?\n', ""
$content = $content -replace "/// Versioned event v2:", "// Versioned event v2:"

$error_fixes = @"
    TransferFailed = 31,
    AlreadyAtTargetVersion = 32,
    MigrationDowngradeNotAllowed = 33,
    AdminRotationSameAddress = 34,
    AdminRotationPending = 35,
    ContractPaused = 36,
    BlacklistSizeLimitExceeded = 37,
    NoAdminRotationPending = 38,
    UnauthorizedRotationAccept = 39,
"@
$content = [regex]::Replace($content, 'TransferFailed = 30,.*?AdminRotationPending = 34,', $error_fixes, [System.Text.RegularExpressions.RegexOptions]::Singleline)

$content = $content -replace '    InvestmentConstraints\(OfferingId\),', "    InvestmentConstraints(OfferingId),`r`n    PaymentTokenDecimals(OfferingId),`r`n    FrozenOffering(OfferingId),"
$content = $content -replace '    ContractFlags,', "    ContractFlags,`r`n    DeployedVersion,"
$content = $content -replace "DataKey::ContractFlags", "DataKey2::ContractFlags"
$content = $content -replace "DataKey::DeployedVersion", "DataKey2::DeployedVersion"
$content = $content -replace "Self::require_valid_period_id\(period_id\)\?;", "if period_id == 0 { return Err(RevoraError::InvalidPeriodId); }"
$content = $content -replace "Self::require_not_offering_frozen\(&env, &offering_id\)\?;", "Self::require_not_frozen(&env)?;"
$content = $content -replace "Self::is_testnet_mode\(env\.clone\(\)\)", "false"
$content = $content -replace "Self::is_event_versioning_enabled\(env\.clone\(\)\)", "false"
$content = $content -replace "_env: &Env", "env: &Env"
$content = $content -replace "fn run_migration_hook\(env: &Env,", "fn run_migration_hook(_env: &Env,"
$content = $content -replace "proposal_duration: u64", "_proposal_duration: u64"

Set-Content -Path "src/lib.rs" -Value $content

$test_content = Get-Content -Path "src/invalid_amount_matrix_tests.rs" -Raw
$test_content = $test_content -replace "client\.get_period_count", "client.get_offering_count"
$test_content = $test_content -replace "make_client\(&env\)", "make_client(env.clone())"
Set-Content -Path "src/invalid_amount_matrix_tests.rs" -Value $test_content
