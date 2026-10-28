<#
Builds the home page (mangapoint.org) into apps/site/dist: any static host can serve that folder.
Nothing is compiled: the page, its pictures, and the app's own fonts, colours and screenshots are copied
next to each other. The mascot's pictures in apps/site/art are made from the app's drawings by
`$env:SITE_ART = 'write'; cargo test -p mangapoint-ui site_art`.

  tools\build-site.ps1
  python tools\serve-web.py --dir apps/site/dist     to look at it
#>
$ErrorActionPreference = 'Stop'

$root = Resolve-Path (Join-Path $PSScriptRoot '..')
$dist = Join-Path $root 'apps\site\dist'

# Only ever delete inside the project.
if (Test-Path $dist) {
    $resolved = (Resolve-Path $dist).Path
    if (-not $resolved.StartsWith($root.Path, [StringComparison]::OrdinalIgnoreCase) -or -not $resolved.EndsWith('apps\site\dist')) {
        throw "Refusing to delete $resolved"
    }
    Remove-Item -Recurse -Force $resolved
}
New-Item -ItemType Directory -Force (Join-Path $dist 'art'), (Join-Path $dist 'icons'), (Join-Path $dist 'img') | Out-Null

$site = Join-Path $root 'apps\site'
Copy-Item (Join-Path $site 'index.html'), (Join-Path $site 'site.css'), (Join-Path $site 'site.js') $dist
Copy-Item (Join-Path $site 'art\*.svg') (Join-Path $dist 'art')
# The app's own style sheets: its colours and type, and its fonts as files (crates/ui/assets/fonts/web;
# the desktop app's copy is inside its CSS, which is too heavy to put in front of a page).
$webFonts = Join-Path $root 'crates\ui\assets\fonts\web'
Copy-Item (Join-Path $root 'crates\ui\css\base.css'), (Join-Path $webFonts 'fonts.css') $dist
New-Item -ItemType Directory -Force (Join-Path $dist 'fonts') | Out-Null
Copy-Item (Join-Path $webFonts 'fonts\*.woff2') (Join-Path $dist 'fonts')
# The clouds and rocks of the app's Search page.
foreach ($name in 'search-cloud-1', 'search-cloud-2', 'search-rock-1', 'search-rock-2') {
    Copy-Item (Join-Path $root "crates\ui\assets\svg\$name.svg") (Join-Path $dist 'art')
}
$icons = Join-Path $root 'apps\web\assets\icons'
Copy-Item (Join-Path $icons 'favicon-32.png'), (Join-Path $icons 'apple-touch-icon.png') (Join-Path $dist 'icons')
# The screenshots as WebP (tools/make-site-images.py makes them from docs/screenshots).
Copy-Item (Join-Path $site 'img\*.webp') (Join-Path $dist 'img')
# How long a browser keeps the files that rarely change (see the file).
Copy-Item (Join-Path $site '_headers') $dist
Copy-Item (Join-Path $root 'docs\banner.png') (Join-Path $dist 'img\banner.png')

Write-Host "built $dist"
