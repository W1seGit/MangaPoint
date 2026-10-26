<#
Serves apps/web/dist on http://127.0.0.1:8080 with the right content types
(application/wasm, text/javascript for modules), the way any static host
would. Nothing else: no uploads, no API.

  tools\serve-web.ps1                 port 8080
  tools\serve-web.ps1 -Port 9000
  tools\serve-web.ps1 -Dir C:\other\folder
#>
param([int]$Port = 8080, [string]$Dir = '')
$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..')
if (-not $Dir) { $Dir = Join-Path $root 'apps\web\dist' }
if (-not (Test-Path (Join-Path $Dir 'index.html'))) {
    throw "No site in $Dir. Build it first: tools\build-web.ps1"
}
$py = Get-Command python -ErrorAction SilentlyContinue
if (-not $py) { $py = Get-Command python3 -ErrorAction Stop }
& $py.Source (Join-Path $PSScriptRoot 'serve-web.py') --dir $Dir --port $Port
