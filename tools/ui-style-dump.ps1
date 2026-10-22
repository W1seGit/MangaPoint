# Records how every element of the desktop app's screens is styled (the computed style of each), to compare
# two builds when the style sheets are reorganised. Uses its own empty data folder, never your library.
#   powershell -File tools/ui-style-dump.ps1 -Exe <path to mangapoint.exe> -Out <file.json> [-Port 9334] [-ShotsDir <folder>]
#   node tools/web-e2e/style-dump.mjs --compare <before.json> <after.json>
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Out,
    [int]$Port = 9334,
    [string]$ShotsDir = ''
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'cdp.ps1')

$data = Join-Path ([IO.Path]::GetTempPath()) ("mangapoint-styles-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $data | Out-Null
[IO.File]::WriteAllText((Join-Path $data 'settings.json'), '{"setup_step":"Done","readers_listed":true,"device":"xteink-x4-pro","custom_devices":[{"id":"xteink-x4-pro","name":"Xteink X4 Pro","width":480,"height":800}]}')
$env:MANGAPOINT_HOME = $data
$env:MANGAPOINT_DEBUG_PORT = "$Port"
$app = Start-Process -FilePath $Exe -PassThru -WindowStyle Hidden
$result = [ordered]@{}
if ($ShotsDir) { New-Item -ItemType Directory -Path $ShotsDir -Force | Out-Null }

# The same list of properties as tools/web-e2e/style-dump.mjs.
$props = (Get-Content (Join-Path $PSScriptRoot 'web-e2e\style-dump.mjs') -Raw) -replace '(?s).*?const PROPS = \[(.*?)\];.*', '$1'
$dumpJs = @"
(() => {
  for (const a of document.getAnimations()) { try { a.finish(); } catch (e) { a.cancel(); } }
  const props = [$props];
  const path = (el) => {
    const parts = [];
    while (el && el.nodeType === 1 && el !== document.documentElement) {
      const parent = el.parentElement;
      const i = parent ? [...parent.children].filter((c) => c.tagName !== 'STYLE' && c.tagName !== 'SCRIPT').indexOf(el) : 0;
      parts.unshift(el.tagName.toLowerCase() + (el.id ? '#' + el.id : '') + ':' + i);
      el = parent;
    }
    return parts.join('>');
  };
  const res = {};
  for (const el of document.querySelectorAll('body *')) {
    if (el.closest('svg') && el.tagName.toLowerCase() !== 'svg') continue;
    if (el.tagName === 'STYLE' || el.tagName === 'SCRIPT') continue;
    const cs = getComputedStyle(el);
    const o = {};
    for (const p of props) o[p] = cs.getPropertyValue(p);
    res[path(el)] = o;
    for (const pseudo of ['::before', '::after']) {
      const ps = getComputedStyle(el, pseudo);
      if (ps.content && ps.content !== 'none' && ps.content !== 'normal') {
        const q = {};
        for (const p of props) q[p] = ps.getPropertyValue(p);
        res[path(el) + pseudo] = q;
      }
    }
  }
  return JSON.stringify(res);
})()
"@

function Dump([string]$Name) {
    Start-Sleep -Milliseconds 700
    $result[$Name] = (Eval $dumpJs) | ConvertFrom-Json
    if ($ShotsDir) { Save-Shot (Join-Path $ShotsDir "$Name.png") }
    Write-Host "$Name"
}
function Go([string]$Label) {
    Eval "(() => { const b = [...document.querySelectorAll('.nav-item')].find(b => new RegExp('$Label', 'i').test(b.innerText)); if (b) b.click(); return !!b; })()" | Out-Null
    Start-Sleep -Milliseconds 600
}

try {
    for ($i = 0; $i -lt 60; $i++) {
        try { Connect-App $Port; break } catch { Start-Sleep -Milliseconds 500 }
    }
    Wait-Until "document.querySelector('.nav') !== null" 300 'the app window'
    Send-Cdp 'Emulation.setDeviceMetricsOverride' @{ width = 1400; height = 900; deviceScaleFactor = 1; mobile = $false } | Out-Null
    Start-Sleep -Milliseconds 800
    Dump 'search'
    Go 'library'; Dump 'library'
    Go 'export'
    Wait-Until "document.querySelectorAll('.px-panel-head').length >= 3" 30 'the panels'
    Start-Sleep -Milliseconds 2500
    Dump 'export'
    Eval "document.querySelector('.zoom-chevron')?.click()" | Out-Null
    Dump 'export-zoom-menu'
    Eval "document.querySelector('.menu-scrim')?.click()" | Out-Null
    Go 'tasks|downloads'; Dump 'tasks'
    Go 'sources'; Dump 'sources'
    Go 'settings'; Dump 'settings'
    $sections = Eval "document.querySelectorAll('.settings-nav button, .sx-side button').length"
    for ($s = 0; $s -lt $sections; $s++) {
        Eval "document.querySelectorAll('.settings-nav button, .sx-side button')[$s].click()" | Out-Null
        Dump "settings-$s"
    }
    Eval "document.querySelectorAll('.settings-nav button, .sx-side button')[0].click()" | Out-Null
    Start-Sleep -Milliseconds 500
    Eval "(() => { const b = [...document.querySelectorAll('.form-footer .btn.danger')][0]; if (b) b.click(); })()" | Out-Null
    Dump 'modal'
    $json = $result | ConvertTo-Json -Depth 6 -Compress
    [IO.File]::WriteAllText($Out, $json)
    Write-Host "written $Out"
} finally {
    try { Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue } catch {}
    # A forced window close bypasses Rust Drop; stop only this test's engine runtime.
    $testRoot = [IO.Path]::GetFullPath($data).TrimEnd('\') + '\'
    Get-CimInstance Win32_Process -Filter "name = 'java.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($testRoot, [StringComparison]::OrdinalIgnoreCase) } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Milliseconds 500
    if ($data.StartsWith([IO.Path]::GetTempPath())) { Remove-Item -Recurse -Force $data -ErrorAction SilentlyContinue }
}
