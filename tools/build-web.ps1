<#
Builds the website into web/dist (any static host can serve that folder).

  tools\build-web.ps1               the page + the converter
  tools\build-web.ps1 -NoEngine     only the page (no export; faster)
  $env:CARGO_TARGET_DIR = '...'     where cargo builds (default: .\target)

Needs: Rust with the wasm32-unknown-unknown target, wasm-bindgen-cli 0.2.129
(cargo install wasm-bindgen-cli --version 0.2.129), and Node 18 or newer.
#>
param([switch]$NoEngine)
$ErrorActionPreference = 'Stop'

$root = Resolve-Path (Join-Path $PSScriptRoot '..')
Set-Location $root
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
$dist = Join-Path $root 'web\dist'

$want = '0.2.129'
$have = $null
try { $have = ((& wasm-bindgen --version) -split ' ')[1] } catch { }
if ($have -ne $want) {
    throw "wasm-bindgen $want is needed (found: $have). Install it with: cargo install wasm-bindgen-cli --version $want"
}

function Run($exe, [string[]]$arguments) {
    & $exe @arguments
    if ($LASTEXITCODE -ne 0) { throw "$exe failed ($LASTEXITCODE)" }
}

# Only ever delete inside the project.
if (Test-Path $dist) {
    $resolved = (Resolve-Path $dist).Path
    if (-not $resolved.StartsWith($root.Path, [StringComparison]::OrdinalIgnoreCase) -or -not $resolved.EndsWith('web\dist')) {
        throw "Refusing to delete $resolved"
    }
    Remove-Item -Recurse -Force $resolved
}
New-Item -ItemType Directory -Force (Join-Path $dist 'css'), (Join-Path $dist 'pkg') | Out-Null

Write-Host '== page (Rust, Dioxus) =='
Run cargo @('build', '-p', 'mangapoint-web', '--target', 'wasm32-unknown-unknown', '--release')
Run wasm-bindgen @('--target', 'web', '--no-typescript', '--out-dir', (Join-Path $dist 'pkg'), '--out-name', 'mangapoint_web',
    (Join-Path $target 'wasm32-unknown-unknown\release\mangapoint_web.wasm'))

if (-not $NoEngine) {
    Write-Host '== converter (Web Worker) =='
    Run cargo @('build', '-p', 'web-engine', '--target', 'wasm32-unknown-unknown', '--release')
    New-Item -ItemType Directory -Force (Join-Path $dist 'engine') | Out-Null
    Run wasm-bindgen @('--target', 'web', '--no-typescript', '--out-dir', (Join-Path $dist 'engine'), '--out-name', 'web_engine',
        (Join-Path $target 'wasm32-unknown-unknown\release\web_engine.wasm'))
}

if (Get-Command wasm-opt -ErrorAction SilentlyContinue) {
    Get-ChildItem -Path $dist -Recurse -Filter '*_bg.wasm' | ForEach-Object {
        & wasm-opt -O2 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals -o "$($_.FullName).opt" $_.FullName
        if ($LASTEXITCODE -eq 0) { Move-Item -Force "$($_.FullName).opt" $_.FullName }
    }
}

Write-Host '== site files =='
Copy-Item (Join-Path $root 'web\index.html'), (Join-Path $root 'web\manifest.webmanifest') $dist
# Hosts that show a page for unknown addresses (GitHub Pages) show the app at /export and the rest.
Copy-Item (Join-Path $root 'web\index.html') (Join-Path $dist '404.html')
Copy-Item -Recurse (Join-Path $root 'web\js') (Join-Path $dist 'js')
Remove-Item -Recurse -Force (Join-Path $dist 'js\test') -ErrorAction SilentlyContinue
Copy-Item -Recurse (Join-Path $root 'web\icons') (Join-Path $dist 'icons')
Copy-Item (Join-Path $root 'web\css\web.css') (Join-Path $dist 'css')
# The desktop app's own style sheets, unchanged (one source of truth for the look).
foreach ($css in 'style.css', 'import.css', 'reader.css', 'editor-controls.css', 'page-editor.css') {
    Copy-Item (Join-Path $root "apps\desktop\src\$css") (Join-Path $dist 'css')
}

# The question-mark tips are the desktop app's own script too.
Copy-Item (Join-Path $root 'apps\desktop\src\js\tooltip.js') (Join-Path $dist 'js')

Run node @((Join-Path $root 'tools\web-precache.mjs'), $dist)
Write-Host "built: $dist"
