# Quick Check Script
# Runs fast checks without full compilation

Write-Host "Quick Check - Multisig Gas Tests" -ForegroundColor Cyan
Write-Host "=================================" -ForegroundColor Cyan
Write-Host ""

# Check if the test file exists
$testFile = "src\test_multisig_gas.rs"
if (Test-Path $testFile) {
    Write-Host "Check 1: Test file exists" -ForegroundColor Green
} else {
    Write-Host "Check 1: Test file missing" -ForegroundColor Red
    exit 1
}

# Check if module is registered in lib.rs
Write-Host ""
$libContent = Get-Content "src\lib.rs" -Raw
if ($libContent -match "mod test_multisig_gas") {
    Write-Host "Check 2: Module registered in lib.rs" -ForegroundColor Green
} else {
    Write-Host "Check 2: Module not registered" -ForegroundColor Red
    exit 1
}

# Count test functions
Write-Host ""
$testContent = Get-Content $testFile -Raw
$testMatches = [regex]::Matches($testContent, "#\[test\]")
$testCount = $testMatches.Count
Write-Host "Check 3: Found $testCount test functions" -ForegroundColor Green

# Summary
Write-Host ""
Write-Host "=================================" -ForegroundColor Cyan
Write-Host "Quick check complete!" -ForegroundColor Green
Write-Host ""
Write-Host "Next steps:" -ForegroundColor Yellow
Write-Host "  1. Install Visual Studio Build Tools with C++" -ForegroundColor Gray
Write-Host "  2. Run: .\verify_ci.ps1" -ForegroundColor Gray
Write-Host "  3. Or run: cargo test test_multisig_gas" -ForegroundColor Gray
Write-Host ""
Write-Host "See CLI_CI_STATUS.md for detailed instructions" -ForegroundColor Gray
Write-Host ""
