[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$IsCI = $env:CI -in @("1","true","True")
$CheckMode = $IsCI -or ($env:CHECK_MODE -in @("1","true","True"))

function Invoke-Fmt {
  if ($CheckMode) {
    Write-Host "==> cargo fmt --all -- --check"
    cargo fmt --all -- --check
  } else {
    Write-Host "==> cargo fmt --all"
    cargo fmt --all
  }
}

function Invoke-Clippy {
  Write-Host "==> cargo clippy --workspace --all-features -- -D warnings"
  cargo clippy --workspace --all-features -- -D warnings
}

function Invoke-Test {
  Write-Host "==> cargo test --workspace --all-features -- --nocapture"
  $env:RUST_BACKTRACE = "1"
  cargo test --workspace --all-features -- --nocapture
}

Invoke-Fmt
Invoke-Clippy
Invoke-Test
Write-Host "✓ All good."
