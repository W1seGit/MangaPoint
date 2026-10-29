<#
Builds the website into apps/web/dist (any static host can serve that folder).

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
$dist = Join-Path $root 'apps\web\dist'

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
    if (-not $resolved.StartsWith($root.Path, [StringComparison]::OrdinalIgnoreCase) -or -not $resolved.EndsWith('apps\web\dist')) {
        throw "Refusing to delete $resolved"
    }
    Remove-Item -Recurse -Force $resolved
}
New-Item -ItemType Directory -Force (Join-Path $dist 'css'), (Join-Path $dist 'pkg') | Out-Null

Write-Host '== page (Rust, Dioxus) =='
Run cargo @('build', '-p', 'mangapoint-web', '--target', 'wasm32-unknown-unknown', '--profile', 'web')
Run wasm-bindgen @('--target', 'web', '--no-typescript', '--out-dir', (Join-Path $dist 'pkg'), '--out-name', 'mangapoint_web',
    (Join-Path $target 'wasm32-unknown-unknown\web\mangapoint_web.wasm'))

if (-not $NoEngine) {
    Write-Host '== converter (Web Worker) =='
    Run cargo @('build', '-p', 'web-engine', '--target', 'wasm32-unknown-unknown', '--profile', 'web-engine')
    New-Item -ItemType Directory -Force (Join-Path $dist 'engine') | Out-Null
    Run wasm-bindgen @('--target', 'web', '--no-typescript', '--out-dir', (Join-Path $dist 'engine'), '--out-name', 'web_engine',
        (Join-Path $target 'wasm32-unknown-unknown\web-engine\web_engine.wasm'))
}

Write-Host '== site files =='
Copy-Item (Join-Path $root 'apps\web\assets\index.html'), (Join-Path $root 'apps\web\assets\manifest.webmanifest') $dist
# Hosts that show a page for unknown addresses (GitHub Pages) show the app at /export and the rest.
Copy-Item (Join-Path $root 'apps\web\assets\index.html') (Join-Path $dist '404.html')
Copy-Item -Recurse (Join-Path $root 'apps\web\js') (Join-Path $dist 'js')
Remove-Item -Recurse -Force (Join-Path $dist 'js\test') -ErrorAction SilentlyContinue
Copy-Item -Recurse (Join-Path $root 'apps\web\assets\icons') (Join-Path $dist 'icons')
# The style sheets, each folder's files in the order of its ORDER file: css/ui.css is the look shared
# with the desktop app (crates/ui/css), css/web.css what a browser and a phone add (apps/web/css).
# The desktop app holds its fonts inside fonts.css; the website uses real font files instead (a third of
# the size, cached on their own, and a script nobody reads is never downloaded): crates/ui/assets/fonts/web.
function Join-Css([string]$Folder, [string]$Out, [string]$First = '', [string]$Skip = '') {
    $names = Get-Content (Join-Path $Folder 'ORDER') | Where-Object { $_.Trim() -and $_.Trim() -ne $Skip }
    $parts = @($names | ForEach-Object { [IO.File]::ReadAllText((Join-Path $Folder $_.Trim())) })
    if ($First) { $parts = @([IO.File]::ReadAllText($First)) + $parts }
    [IO.File]::WriteAllText($Out, ($parts -join "`n"), (New-Object System.Text.UTF8Encoding $false))
}
$webFonts = Join-Path $root 'crates\ui\assets\fonts\web'
New-Item -ItemType Directory -Force (Join-Path $dist 'css\fonts') | Out-Null
Copy-Item (Join-Path $webFonts 'fonts\*.woff2') (Join-Path $dist 'css\fonts')
Join-Css (Join-Path $root 'crates\ui\css') (Join-Path $dist 'css\ui.css') (Join-Path $webFonts 'fonts.css') 'fonts.css'
Join-Css (Join-Path $root 'apps\web\css') (Join-Path $dist 'css\web.css')

# The page scripts shared with the desktop app (crates/ui/js), loaded by name from js/ui/.
New-Item -ItemType Directory -Force (Join-Path $dist 'js\ui') | Out-Null
Copy-Item (Join-Path $root 'crates\ui\js\*.js') (Join-Path $dist 'js\ui')

Run node @((Join-Path $root 'tools\web-precache.mjs'), $dist)
Write-Host "built: $dist"
