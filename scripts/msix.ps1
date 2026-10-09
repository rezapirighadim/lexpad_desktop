# Packs the release build into the Microsoft Store's MSIX, unsigned (the
# Store signs what it publishes). Runs on Windows with the Windows SDK
# (makeappx, makepri), in CI's release-windows job right after
# `pnpm tauri build`, before anything else rebuilds the exe.
#
#   pwsh scripts/msix.ps1 [-Exe src-tauri/target/release/lexpad-desktop.exe] [-OutDir src-tauri/target/release/bundle/msix]
#
# Writes <OutDir>/Lexpad_<version>_x64.msix and prints its path. The manifest
# is msix/AppxManifest.xml; the logos are msix/Assets (scripts/icons.py).
param(
  [string]$Exe = 'src-tauri/target/release/lexpad-desktop.exe',
  [string]$OutDir = 'src-tauri/target/release/bundle/msix'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

# The newest Windows SDK's x64 tools.
$kits = 'C:\Program Files (x86)\Windows Kits\10\bin'
$bin = Get-ChildItem $kits -Directory | Where-Object { $_.Name -match '^10\.' -and (Test-Path (Join-Path $_.FullName 'x64\makeappx.exe')) } |
  Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
if (-not $bin) { throw "makeappx.exe not found under $kits" }
$tools = Join-Path $bin.FullName 'x64'
Write-Host "Windows SDK tools: $tools"

$version = (Get-Content (Join-Path $root 'package.json') -Raw | ConvertFrom-Json).version
if ($version -notmatch '^\d+\.\d+\.\d+$') { throw "package.json version '$version' is not major.minor.patch" }
$msixVersion = "$version.0"

$exePath = Resolve-Path $Exe
$layout = Join-Path ([IO.Path]::GetTempPath()) "lexpad-msix-$([guid]::NewGuid())"
New-Item -ItemType Directory -Path $layout | Out-Null
try {
  Copy-Item $exePath (Join-Path $layout 'Lexpad.exe')
  Copy-Item (Join-Path $root 'msix/Assets') (Join-Path $layout 'Assets') -Recurse
  $manifest = (Get-Content (Join-Path $root 'msix/AppxManifest.xml') -Raw).Replace('__VERSION__', $msixVersion)
  Set-Content -Path (Join-Path $layout 'AppxManifest.xml') -Value $manifest -Encoding utf8NoBOM

  # resources.pri: lets Windows pick each logo's scale and target size.
  $pricfg = Join-Path ([IO.Path]::GetTempPath()) "lexpad-priconfig-$([guid]::NewGuid()).xml"
  & (Join-Path $tools 'makepri.exe') createconfig /cf $pricfg /dq en-US /pv 10.0.0 /o
  if ($LASTEXITCODE) { throw "makepri createconfig failed ($LASTEXITCODE)" }
  # One package, one resources.pri: without this, makepri splits each scale
  # into resources.scale-NNN.pri for resource packages a bundle would carry,
  # and a single .msix would lose every logo but scale-100.
  [xml]$cfg = Get-Content $pricfg
  $cfg.SelectNodes('//packaging') | ForEach-Object { [void]$_.ParentNode.RemoveChild($_) }
  $cfg.Save($pricfg)
  & (Join-Path $tools 'makepri.exe') new /pr $layout /cf $pricfg /mn (Join-Path $layout 'AppxManifest.xml') /of (Join-Path $layout 'resources.pri') /o
  if ($LASTEXITCODE) { throw "makepri new failed ($LASTEXITCODE)" }
  if (Get-ChildItem $layout -Filter 'resources.*.pri') { throw 'makepri split the resources; expected one resources.pri' }
  Remove-Item $pricfg

  New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
  $out = Join-Path (Resolve-Path $OutDir) "Lexpad_${version}_x64.msix"
  & (Join-Path $tools 'makeappx.exe') pack /d $layout /p $out /o
  if ($LASTEXITCODE) { throw "makeappx pack failed ($LASTEXITCODE)" }
  $size = (Get-Item $out).Length
  Write-Host ("MSIX {0} ({1:N1} MB), version {2}" -f $out, ($size / 1MB), $msixVersion)
  $out
} finally {
  Remove-Item $layout -Recurse -Force -ErrorAction SilentlyContinue
}
