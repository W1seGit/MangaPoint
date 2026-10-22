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
# A tiny imported book exercises the real library and reader without network data.
$pages = Join-Path $data 'library/smoke-book/c1'
New-Item -ItemType Directory -Path $pages -Force | Out-Null
Add-Type -AssemblyName System.Drawing
$picture = New-Object Drawing.Bitmap 240,480
$graphics = [Drawing.Graphics]::FromImage($picture)
try {
    $graphics.Clear([Drawing.Color]::White)
    $graphics.FillRectangle([Drawing.Brushes]::Gray, 24, 24, 190, 400)
    $picture.Save((Join-Path $pages '00001.img'), [Drawing.Imaging.ImageFormat]::Png)
} finally { $graphics.Dispose(); $picture.Dispose() }
$book = @{ id='smoke-book'; title='Smoke book'; chapters=@(@{ key='c1'; number=1; name='Chapter 1'; source_id='import'; pages=1; bytes=1000 }); imported=@{ files=@('fixture.png'); at=1; books=@() } }
[IO.File]::WriteAllText((Join-Path $data 'library.json'), (@{ series=@($book); reading=@{} } | ConvertTo-Json -Depth 8))
$env:MANGAPOINT_HOME = $data
$env:MANGAPOINT_DEBUG_PORT = "$Port"
$exePath = if ([IO.Path]::IsPathRooted($Exe)) { $Exe } else { Join-Path $root $Exe }
$app = Start-Process -FilePath $exePath -PassThru -WindowStyle Hidden
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
    Check 'reader form guards unsaved edits and can discard them' {
        Mouse-Click '.nav-item' '^Settings'
        Wait-Until "document.querySelector('.title-input') !== null" 15 'the reader form'
        Set-Input '.title-input' 'Temporary reader name'
        Mouse-Click '.nav-item' '^Library'
        Wait-Until "document.querySelector('#unsaved-prompt') !== null" 10 'the unsaved prompt'
        Mouse-Click '#unsaved-keep'
        Wait-Until "!document.querySelector('#unsaved-prompt')" 5 'keep editing'
        Mouse-Click '.nav-item' '^Library'
        Wait-Until "document.querySelector('#unsaved-prompt') !== null" 10 'the second prompt'
        Mouse-Click '#unsaved-discard'
        Wait-Until "document.querySelector('.poster') !== null" 10 'the library card'
    }
    Check 'library cards select and deselect through the shared action bar' {
        Mouse-Click '.poster-check'
        Wait-Until "document.querySelector('#bar-count')?.textContent === '1 selected'" 5 'the selection bar'
        Mouse-Click '#bar-select-all'
        Wait-Until "!document.querySelector('#action-bar')" 5 'selection cleared'
    }
    Check 'reader bars, settings and zoom work with a saved local page' {
        Mouse-Click '.poster-read'
        Wait-Until "document.querySelector('#reader') !== null" 15 'the reader'
        Wait-Until "[...document.querySelectorAll('.reader img')].some(i => i.complete && i.naturalWidth > 0)" 15 'the saved page'
        Mouse-Click '#reader-view-button'
        Wait-Until "document.querySelector('#reader-settings') !== null" 5 'reading settings'
        Mouse-Click '#reader-zoom-toggle'
        Wait-Until "document.querySelector('#reader-zoom-list') !== null" 5 'the zoom choices'
        Mouse-Click '#reader-zoom-list button' '^150%$'
        Wait-Until "document.querySelector('#reader-zoom-box')?.value === '150%'" 5 '150 percent zoom'
        Mouse-Click '#reader-bars-always'
        Wait-Until "document.querySelector('#reader').classList.contains('pinned')" 5 'pinned reader bars'
        Mouse-Click '#reader-settings-close'
        Mouse-Click '#reader-back'
        Wait-Until "!document.querySelector('#reader') && document.querySelector('.poster')" 10 'return to library'
    }
    Check 'Ctrl+K goes to the search' {
        Send-Cdp 'Input.dispatchKeyEvent' @{ type='keyDown'; key='k'; code='KeyK'; modifiers=2; windowsVirtualKeyCode=75 } | Out-Null
        Send-Cdp 'Input.dispatchKeyEvent' @{ type='keyUp'; key='k'; code='KeyK'; modifiers=2; windowsVirtualKeyCode=75 } | Out-Null
        Wait-Until "document.querySelector('.nav-item.active').innerText.trim().startsWith('Search')" 10 'the Search page'
    }
}
finally {
    if ($app -and -not $app.HasExited) { Stop-Process -Id $app.Id -Force }
    # A forced window close bypasses Rust Drop; stop only this test's engine runtime.
    $testRoot = [IO.Path]::GetFullPath($data).TrimEnd('\') + '\'
    Get-CimInstance Win32_Process -Filter "name = 'java.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($testRoot, [StringComparison]::OrdinalIgnoreCase) } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    if ($data -like (Join-Path ([IO.Path]::GetTempPath()) 'mangapoint-smoke-*')) { Remove-Item -LiteralPath $data -Recurse -Force -ErrorAction SilentlyContinue }
}
if ($failures.Count) { Write-Host "$($failures.Count) check(s) failed"; exit 1 }
Write-Host 'all checks passed'
