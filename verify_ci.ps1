# CI Verification Script
# This script runs all the checks that GitHub Actions CI will run
# Run this before pushing to ensure CI will pass

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  Revora Contracts - CI Verification" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

$ErrorCount = 0

# Check if cargo is available
Write-Host "[0/4] Checking Rust installation..." -ForegroundColor Yellow
try {
    $cargoVersion = cargo --version 2>&1
    Write-Host "  ✓ Cargo found: $cargoVersion" -ForegroundColor Green
} catch {
    Write-Host "  ✗ Cargo not found. Please install Rust from https://rustup.rs/" -ForegroundColor Red
    exit 1
}

# Check if MSVC linker is available
Write-Host ""
Write-Host "[0/4] Checking MSVC linker..." -ForegroundColor Yellow
$linkExe = Get-Command link.exe -ErrorAction SilentlyContinue
if ($linkExe) {
    Write-Host "  ✓ MSVC linker found: $($linkExe.Source)" -ForegroundColor Green
} else {
    Write-Host "  ✗ MSVC linker (link.exe) not found" -ForegroundColor Red
    Write-Host "    Install Visual Studio Build Tools with C++ support" -ForegroundColor Yellow
    Write-Host "    Or use: rustup target add x86_64-pc-windows-gnu" -ForegroundColor Yellow
    Write-Host "    See CLI_CI_STATUS.md for details" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "  Attempting to continue anyway..." -ForegroundColor Yellow
}

Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  Running CI Checks" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

# 1. Format Check
Write-Host ""
Write-Host "[1/4] Running format check (cargo fmt)..." -ForegroundColor Yellow
Write-Host "  Command: cargo fmt --all -- --check" -ForegroundColor Gray
$fmtResult = cargo fmt --all -- --check 2>&1
if ($LASTEXITCODE -eq 0) {
    Write-Host "  ✓ Format check passed" -ForegroundColor Green
} else {
    Write-Host "  ✗ Format check failed" -ForegroundColor Red
    Write-Host "  Run 'cargo fmt --all' to fix formatting" -ForegroundColor Yellow
    Write-Host $fmtResult -ForegroundColor Red
    $ErrorCount++
}

# 2. Clippy Check
Write-Host ""
Write-Host "[2/4] Running clippy lint check..." -ForegroundColor Yellow
Write-Host "  Command: cargo clippy --all-targets --all-features -- -D warnings" -ForegroundColor Gray
Write-Host "  (This may take a few minutes on first run...)" -ForegroundColor Gray
$clippyResult = cargo clippy --all-targets --all-features -- -D warnings 2>&1
if ($LASTEXITCODE -eq 0) {
    Write-Host "  ✓ Clippy check passed" -ForegroundColor Green
} else {
    Write-Host "  ✗ Clippy check failed" -ForegroundColor Red
    Write-Host $clippyResult -ForegroundColor Red
    $ErrorCount++
}

# 3. Build
Write-Host ""
Write-Host "[3/4] Running build (release mode)..." -ForegroundColor Yellow
Write-Host "  Command: cargo build --release" -ForegroundColor Gray
Write-Host "  (This may take a few minutes...)" -ForegroundColor Gray
$buildResult = cargo build --release 2>&1
if ($LASTEXITCODE -eq 0) {
    Write-Host "  ✓ Build passed" -ForegroundColor Green
} else {
    Write-Host "  ✗ Build failed" -ForegroundColor Red
    Write-Host $buildResult -ForegroundColor Red
    $ErrorCount++
}

# 4. Test
Write-Host ""
Write-Host "[4/4] Running tests..." -ForegroundColor Yellow
Write-Host "  Command: cargo test -- --test-threads=1" -ForegroundColor Gray
Write-Host "  (This may take a few minutes...)" -ForegroundColor Gray
$env:RUST_BACKTRACE = "full"
$testResult = cargo test -- --test-threads=1 2>&1
if ($LASTEXITCODE -eq 0) {
    Write-Host "  ✓ All tests passed" -ForegroundColor Green
} else {
    Write-Host "  ✗ Tests failed" -ForegroundColor Red
    Write-Host $testResult -ForegroundColor Red
    $ErrorCount++
}

# Summary
Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  Summary" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

if ($ErrorCount -eq 0) {
    Write-Host "  ✓ All CI checks passed!" -ForegroundColor Green
    Write-Host "  Your code is ready to push." -ForegroundColor Green
    exit 0
} else {
    Write-Host "  ✗ $ErrorCount check(s) failed" -ForegroundColor Red
    Write-Host "  Please fix the errors before pushing." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "  Common fixes:" -ForegroundColor Yellow
    Write-Host "    - Format: cargo fmt --all" -ForegroundColor Gray
    Write-Host "    - Clippy: Review and fix warnings" -ForegroundColor Gray
    Write-Host "    - Build: Check compilation errors" -ForegroundColor Gray
    Write-Host "    - Tests: Review test failures" -ForegroundColor Gray
    exit 1
}
