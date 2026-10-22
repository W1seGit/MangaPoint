# Drives a real build of the app with real mouse clicks and checks that the Export screen responds.
# It uses its own empty data folder, never your library.
#   cargo build -p mangapoint
#   powershell -File tools/ui-smoke.ps1 [-Exe target\debug\mangapoint.exe] [-Port 9333]
param(
    [string]$Exe = 'target\debug\mangapoint.exe',
    [int]$Port = 9333
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'cdp.ps1')

$data = Join-Path ([IO.Path]::GetTempPath()) ("mangapoint-smoke-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $data | Out-Null
# A fresh folder would start the first-run setup; start past it, with one reader.
[IO.File]::WriteAllText((Join-Path $data 'settings.json'), '{"setup_step":"Done","readers_listed":true,"device":"xteink-x4-pro","custom_devices":[{"id":"xteink-x4-pro","name":"Xteink X4 Pro","width":480,"height":800}]}')
$env:MANGAPOINT_HOME = $data
$env:MANGAPOINT_DEBUG_PORT = "$Port"
$exePath = if ([IO.Path]::IsPathRooted($Exe)) { $Exe } else { Join-Path $root $Exe }
$app = Start-Process -FilePath $exePath -PassThru
$failures = @()

function Check([string]$Name, [scriptblock]$Test) {
    try { & $Test; Write-Host "ok    $Name" }
    catch { Write-Host "FAIL  $Name  ($($_.Exception.Message))"; $script:failures += $Name }
}

try {
    for ($i = 0; $i -lt 60; $i++) {
        try { Connect-App $Port; break } catch { Start-Sleep -Milliseconds 500 }
    }
    Wait-Until "document.querySelector('.nav') !== null" 300 'the app window'

    Check 'the Export page opens on the sample pages' {
        Mouse-Click '.nav-item' 'export'
        Wait-Until "document.querySelector('.export-screen') !== null" 30 'the Export screen'
        Wait-Until "document.querySelectorAll('.px-panel-head').length >= 3" 30 'the panels'
    }
    Check 'a panel folds when its header is clicked' {
        $before = Eval "document.querySelectorAll('.px-panel.collapsed').length"
        Mouse-Click '.px-panel-head' 'presets'
        Start-Sleep -Milliseconds 600
        $after = Eval "document.querySelectorAll('.px-panel.collapsed').length"
        if ($after -le $before) { throw "still $before collapsed panels" }
    }
    Check 'the Export page lists the readers, with no size boxes, and New device goes to Settings' {
        Wait-Until "document.querySelectorAll('.px-scroll .px-preset').length >= 1" 15 'the reader list'
        if (Eval "document.querySelectorAll('.size-boxes').length") { throw 'size boxes are on the Export screen' }
        Mouse-Click '.px-new' 'New device'
        Wait-Until "document.querySelector('.dim-box') !== null" 15 'the reader page in Settings'
        Mouse-Click '.nav-item' 'export'
        Wait-Until "document.querySelector('.export-screen') !== null" 30 'the Export screen'
    }
    Check 'the Export button stays on screen in the smallest window' {
        Send-Cdp 'Emulation.setDeviceMetricsOverride' @{ width = 1280; height = 720; deviceScaleFactor = 1; mobile = $false } | Out-Null
        Start-Sleep -Milliseconds 600
        try {
            $r = Eval "(() => { const b = document.querySelector('.px-footer .btn').getBoundingClientRect(); return [b.top, b.bottom, innerHeight]; })()"
            if ([double]$r[0] -lt 0 -or [double]$r[1] -gt [double]$r[2] + 0.5) { throw "the button spans $($r[0])-$($r[1]) in a $($r[2]) px window" }
        }
        finally { Send-Cdp 'Emulation.clearDeviceMetricsOverride' @{} | Out-Null; Start-Sleep -Milliseconds 300 }
    }
    Check 'a column bar can be dragged' {
        $w0 = Eval "document.querySelector('.export-side.left').getBoundingClientRect().width"
        $pt = Eval "(() => { const r = document.querySelector('.px-split-v[data-split=left]').getBoundingClientRect(); return [r.left + 1.5, r.top + 200]; })()"
        $x = [double]$pt[0]; $y = [double]$pt[1]
        Send-Cdp 'Input.dispatchMouseEvent' @{ type='mouseMoved'; x=$x; y=$y } | Out-Null
        Send-Cdp 'Input.dispatchMouseEvent' @{ type='mousePressed'; x=$x; y=$y; button='left'; clickCount=1 } | Out-Null
        Send-Cdp 'Input.dispatchMouseEvent' @{ type='mouseMoved'; x=($x+60); y=$y; buttons=1 } | Out-Null
        Send-Cdp 'Input.dispatchMouseEvent' @{ type='mouseReleased'; x=($x+60); y=$y; button='left'; clickCount=1 } | Out-Null
        Start-Sleep -Milliseconds 300
        $w1 = Eval "document.querySelector('.export-side.left').getBoundingClientRect().width"
        if ([math]::Abs($w1 - $w0) -lt 20) {
            $hit = Eval "(() => { const e = document.elementFromPoint($x, $y); return (e ? e.className : 'none') + ' bound=' + document.querySelector('.export-screen').dataset.dragBound; })()"
            throw "width stayed $w0 (point hits: $hit)"
        }
    }
    Check 'clicking the chapter still works after dragging' {
        Mouse-Click '.px-chapter-open'
        Start-Sleep -Milliseconds 400
        if (-not (Eval "document.querySelector('.export-screen') !== null")) { throw 'the screen is gone' }
    }
    Check 'Ctrl+K goes to the search' {
        Send-Cdp 'Input.dispatchKeyEvent' @{ type='keyDown'; key='k'; code='KeyK'; modifiers=2; windowsVirtualKeyCode=75 } | Out-Null
        Send-Cdp 'Input.dispatchKeyEvent' @{ type='keyUp'; key='k'; code='KeyK'; modifiers=2; windowsVirtualKeyCode=75 } | Out-Null
        Wait-Until "document.querySelector('.nav-item.active').innerText.trim().startsWith('Search')" 10 'the Search page'
    }
}
finally {
    if ($app -and -not $app.HasExited) { Stop-Process -Id $app.Id -Force }
    if ($data -like (Join-Path ([IO.Path]::GetTempPath()) 'mangapoint-smoke-*')) { Remove-Item -LiteralPath $data -Recurse -Force -ErrorAction SilentlyContinue }
}
if ($failures.Count) { Write-Host "$($failures.Count) check(s) failed"; exit 1 }
Write-Host 'all checks passed'
