<#
Build the website engine as WebAssembly, in the two flavours the site uses:

  <out>\core    the small one the page loads (state file, chapter guessing,
                fingerprints, device list), for --target web
  <out>\worker  the full one the Web Worker loads (adds the converter and the
                file writers), for --target web
  <out>\node    the full one for Node (--target nodejs), used by the smoke test

usage: build-wasm.ps1 [-OutDir <dir>]   (default: .\wasm-out next to this script)
env:   CARGO_TARGET_DIR  where cargo builds (default: <workspace>\target)

Needs the wasm32-unknown-unknown target and wasm-bindgen-cli 0.2.129 (the version
the crate pins). wasm-opt (binaryen) is used only if it is installed.
#>
param([string]$OutDir = (Join-Path $PSScriptRoot 'wasm-out'))
$ErrorActionPreference = 'Stop'

$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$out = (Resolve-Path $OutDir).Path
$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
$wasm = Join-Path $targetDir 'wasm32-unknown-unknown\release\web_engine.wasm'

function Build([string]$label, [string[]]$cargoArgs) {
    # Small code for the page (it only does book-keeping); fast code for the worker.
    $level = if ($label -eq 'core') { 's' } else { '3' }
    Write-Host "== building $label (opt-level $level)"
    $env:CARGO_PROFILE_RELEASE_OPT_LEVEL = $level
    $env:CARGO_PROFILE_RELEASE_LTO = 'true'
    $env:CARGO_PROFILE_RELEASE_CODEGEN_UNITS = '1'
    $env:CARGO_PROFILE_RELEASE_STRIP = 'true'
    Push-Location $root
    try {
        cargo build -p web-engine --target wasm32-unknown-unknown --release @cargoArgs
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($label)" }
    } finally { Pop-Location }
}

function Bindgen([string]$target, [string]$dir) {
    $dest = Join-Path $out $dir
    if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
    wasm-bindgen --target $target --out-dir $dest --out-name web_engine $wasm
    if ($LASTEXITCODE -ne 0) { throw "wasm-bindgen failed ($dir)" }
    if (Get-Command wasm-opt -ErrorAction SilentlyContinue) {
        $file = Join-Path $dest 'web_engine_bg.wasm'
        wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals $file -o "$file.opt"
        if ($LASTEXITCODE -eq 0) { Move-Item -Force "$file.opt" $file }
    }
}

Build 'core' @('--no-default-features', '--features', 'core')
Bindgen 'web' 'core'

Build 'worker' @()
Bindgen 'web' 'worker'
Bindgen 'nodejs' 'node'

Write-Host ''
Write-Host '== sizes (bytes)'
foreach ($d in 'core', 'worker', 'node') {
    $f = Join-Path $out "$d\web_engine_bg.wasm"
    '{0,-8} {1,10}  {2}' -f $d, (Get-Item $f).Length, $f
}
