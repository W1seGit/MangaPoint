# Everything that has to pass before a change to the shared code is done: both apps build, the
# libraries' tests, the website's browser tests and the desktop app driven with real clicks.
#   powershell -File tools/check-all.ps1            # all of it (the browser tests take a while)
#   powershell -File tools/check-all.ps1 -Quick     # builds and unit tests only
#   powershell -File tools/check-all.ps1 -SkipWeb   # no browser tests
#   powershell -File tools/check-all.ps1 -SkipDesktop
# Build output goes to .work/target-desk and .work/target-web beside the checkout when those exist,
# else to the usual target folder.
param(
    [switch]$Quick,
    [switch]$SkipWeb,
    [switch]$SkipDesktop
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$failed = @()

function Step([string]$Name, [scriptblock]$Run) {
    Write-Host "`n== $Name"
    & $Run
    if ($LASTEXITCODE -ne 0) { Write-Host "FAILED: $Name"; $script:failed += $Name }
}

function TargetDir([string]$Name) {
    $dir = Join-Path (Split-Path -Parent $root) $Name
    if (Test-Path $dir) { return $dir }
    return (Join-Path $root 'target')
}

$desk = TargetDir 'target-desk'
$web = TargetDir 'target-web'

Step 'desktop app builds' { $env:CARGO_TARGET_DIR = $desk; cargo build -p mangapoint }
Step 'website builds (wasm32)' { $env:CARGO_TARGET_DIR = $web; cargo check -p mangapoint-web --target wasm32-unknown-unknown }
Step 'unit tests' {
    $env:CARGO_TARGET_DIR = $desk
    cargo test -p library -p converter -p device-profiles -p pipeline -p web-engine -p mangapoint-ui -p mangapoint -p mangapoint-web
}
Step 'web engine parity with the desktop' { $env:CARGO_TARGET_DIR = $desk; cargo test -p web-engine --test parity }

if (-not $Quick) {
    if (-not $SkipWeb) {
        Step 'website build' { $env:CARGO_TARGET_DIR = $web; powershell -NoProfile -File (Join-Path $PSScriptRoot 'build-web.ps1') }
        Step 'website browser tests (every suite, three sizes)' { node (Join-Path $PSScriptRoot 'web-e2e/run-all.mjs') }
    }
    if (-not $SkipDesktop) {
        $exe = Join-Path $desk 'debug\mangapoint.exe'
        Step 'desktop app with real clicks' { powershell -NoProfile -File (Join-Path $PSScriptRoot 'ui-smoke.ps1') -Exe $exe }
    }
}

Write-Host ''
if ($failed.Count) {
    Write-Host "$($failed.Count) step(s) failed:"
    $failed | ForEach-Object { Write-Host "  - $_" }
    exit 1
}
Write-Host 'everything passed'
