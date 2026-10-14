<#
.SYNOPSIS
  Build one ArtCraft app for Windows and collect its .exe files under builds/.

.DESCRIPTION
  Runs exactly the cargo build the app's own packaging/windows/package.ps1 runs (same packages,
  same features, --locked, static CRT), inside apps/<app>, then copies the binaries to:

    builds/<app>/<version>/windows-<arch>/<app>.exe, <app>-cli.exe, BUILD-INFO.json

  Each binary's PE header is checked (machine type, GUI vs console) before it is reported as
  built. With -Msi it then runs the upstream package.ps1 -SkipBuild (needs WiX v5) and writes
  the .msi and portable .zip into the same folder.

  Nothing in apps/<app> is modified except its git-ignored target/ directory.

.EXAMPLE
  scripts\build-app.ps1 -App photocraft
  scripts\build-app.ps1 -App pdfcraft -Arch x64 -Msi
  scripts\build-app.ps1 -App wordcraft -BuildProfile debug
#>
param(
  [Parameter(Mandatory = $true)] [string] $App,
  [ValidateSet('x64', 'x86', 'arm64')] [string] $Arch = 'x64',
  [ValidateSet('release', 'debug')] [string] $BuildProfile = 'release',
  [switch] $Msi
)
. "$PSScriptRoot\lib\common.ps1"

$a = Get-App $App
$dir = Get-AppDir $a
$version = Get-AppVersion $a
$target = switch ($Arch) { 'x64' { 'x86_64-pc-windows-msvc' } 'x86' { 'i686-pc-windows-msvc' } 'arm64' { 'aarch64-pc-windows-msvc' } }
$out = Join-Path $script:RepoRoot "builds\$($a.id)\$version\windows-$Arch"

if (-not (Test-Command cargo)) {
  $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
  if (Test-Path (Join-Path $cargoBin 'cargo.exe')) { $env:PATH = "$cargoBin;$env:PATH" }
  else { throw 'cargo not found. Run scripts\check-env.ps1 and see docs\WINDOWS-SETUP.md.' }
}

Write-Section "$($a.name) $version -> Windows $Arch ($target, $BuildProfile)"

# Same env the upstream packager sets.
$flagVar = 'CARGO_TARGET_' + ($target.ToUpper() -replace '-', '_') + '_RUSTFLAGS'
[Environment]::SetEnvironmentVariable($flagVar, '-C target-feature=+crt-static')
$upper = $a.id.ToUpper()
[Environment]::SetEnvironmentVariable("$($upper)_BUILD_SHA", (git -C $script:RepoRoot rev-parse HEAD))
[Environment]::SetEnvironmentVariable("$($upper)_BUILD_DATE", (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd'))

$cargoArgs = @('build', '--locked', '--target', $target)
if ($BuildProfile -eq 'release') { $cargoArgs += '--release' }
foreach ($p in $a.build.packages) {
  if (Test-Path (Join-Path $dir "apps\$p\Cargo.toml")) { $cargoArgs += @('-p', $p) }
  else { Write-Warning "package $p not present in this upstream revision; skipped" }
}
if ($a.build.features.Count -gt 0) { $cargoArgs += @('--features', ($a.build.features -join ',')) }

$started = Get-Date
Push-Location $dir
try {
  Write-Host "> cargo $($cargoArgs -join ' ')"
  & cargo @cargoArgs
  if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $($a.id) (exit $LASTEXITCODE)" }
} finally { Pop-Location }

$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $dir 'target' }
$binDir = Join-Path $targetDir "$target\$BuildProfile"

function Get-PeHeader([string] $Path) {
  $bytes = [System.IO.File]::ReadAllBytes($Path)
  $pe = [BitConverter]::ToInt32($bytes, 0x3C)
  return @{ Machine = [BitConverter]::ToUInt16($bytes, $pe + 4); Subsystem = [BitConverter]::ToUInt16($bytes, $pe + 0x5C) }
}
$machine = switch ($Arch) { 'x64' { 0x8664 } 'x86' { 0x14C } 'arm64' { 0xAA64 } }

New-Item -ItemType Directory -Force -Path $out | Out-Null
$artifacts = @()
foreach ($exe in $a.build.binaries) {
  $src = Join-Path $binDir $exe
  if (-not (Test-Path $src)) {
    if ($exe -like '*-cli.exe' -and -not (Test-Path (Join-Path $dir "apps\$($exe -replace '\.exe$','')"))) { continue }
    throw "expected binary missing after build: $src"
  }
  $h = Get-PeHeader $src
  if ($h.Machine -ne $machine) { throw "$exe machine 0x$('{0:X}' -f $h.Machine) does not match $Arch" }
  $kind = switch ($h.Subsystem) { 2 { 'gui' } 3 { 'console' } default { "subsystem-$($h.Subsystem)" } }
  Copy-Item $src $out -Force
  $item = Get-Item (Join-Path $out $exe)
  $artifacts += [ordered]@{ file = $exe; subsystem = $kind; bytes = $item.Length; sha256 = (Get-FileHash $item.FullName -Algorithm SHA256).Hash.ToLower() }
  Write-Host ("  {0,-22} {1,-8} {2,8:N1} MB" -f $exe, $kind, ($item.Length / 1MB)) -ForegroundColor Green
}

if ($Msi) {
  if ($BuildProfile -ne 'release') { throw '-Msi requires -BuildProfile release' }
  if (-not (Test-Command wix)) { throw 'WiX v5 not found; MSI packaging skipped. See docs\BUILD.md.' }
  $pwsh = if (Test-Command pwsh) { 'pwsh' } else { 'powershell' }
  $env:DIST = $out
  Push-Location $dir
  try {
    & $pwsh -NoProfile -ExecutionPolicy Bypass -File 'packaging\windows\package.ps1' -Arch $Arch -SkipBuild
    if ($LASTEXITCODE -ne 0) { throw "upstream package.ps1 failed for $($a.id) (exit $LASTEXITCODE)" }
  } finally { Pop-Location; Remove-Item Env:\DIST -ErrorAction SilentlyContinue }
  Get-ChildItem $out -Include *.msi, *.zip -Recurse | ForEach-Object { Write-Host "  $($_.Name)" -ForegroundColor Green }
}

$info = [ordered]@{
  app = $a.id; name = $a.name; version = $version; arch = $Arch; target = $target; profile = $BuildProfile
  monorepoCommit = (git -C $script:RepoRoot rev-parse HEAD); upstreamCommit = $a.upstream.importedCommit
  rustc = (rustc --version); builtAt = (Get-Date).ToUniversalTime().ToString('o')
  seconds = [int]((Get-Date) - $started).TotalSeconds; artifacts = $artifacts
}
$info | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 (Join-Path $out 'BUILD-INFO.json')
Write-Host "Built $($a.id) $version -> $out" -ForegroundColor Green
