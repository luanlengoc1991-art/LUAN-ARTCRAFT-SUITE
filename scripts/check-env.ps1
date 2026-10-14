<#
.SYNOPSIS
  Report whether this Windows machine can build the ArtCraft apps. Changes nothing.

.DESCRIPTION
  Checks: git (+ core.longpaths), rustup/cargo/rustc and the minimum rust-version each app
  declares, the MSVC linker (Visual Studio Build Tools, C++ workload), the Windows SDK
  (rc.exe), and, for MSI packaging only, the .NET SDK and WiX v5.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\check-env.ps1
#>
. "$PSScriptRoot\lib\common.ps1"
$ErrorActionPreference = 'Continue'
$problems = @()

function Report([string] $Name, [bool] $Ok, [string] $Detail, [string] $Fix) {
  $mark = if ($Ok) { '[ OK ]' } else { '[MISS]' }
  $color = if ($Ok) { 'Green' } else { 'Yellow' }
  Write-Host ("{0} {1,-28} {2}" -f $mark, $Name, $Detail) -ForegroundColor $color
  if (-not $Ok) { $script:problems += "$Name -> $Fix" }
}

Write-Section 'Required to build .exe'

$git = Test-Command git
Report 'git' $git $(if ($git) { (git --version) } else { '' }) 'winget install Git.Git'
if ($git) {
  $lp = git -C $script:RepoRoot config core.longpaths
  Report 'git core.longpaths' ($lp -eq 'true') "value: $lp" 'git config core.longpaths true'
}

$cargoExe = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
$hasCargo = (Test-Command cargo) -or (Test-Path $cargoExe)
$rustcVer = $null
if ($hasCargo) {
  if (-not (Test-Command rustc)) { $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH" }
  $rustcVer = (rustc --version) -replace '^rustc (\S+).*', '$1'
}
Report 'Rust (rustup/cargo)' $hasCargo $(if ($rustcVer) { "rustc $rustcVer" } else { 'not installed' }) 'winget install Rustlang.Rustup  (then: rustup default stable-x86_64-pc-windows-msvc)'

if ($rustcVer) {
  $need = (Get-Manifest).apps | ForEach-Object { [version]$_.rustVersion } | Sort-Object -Descending | Select-Object -First 1
  $have = [version](($rustcVer -split '-')[0])
  Report 'rustc >= max rust-version' ($have -ge $need) "have $have, apps need up to $need" 'rustup update stable'
  $targets = rustup target list --installed 2>$null
  Report 'target x86_64-pc-windows-msvc' ($targets -contains 'x86_64-pc-windows-msvc') '' 'rustup target add x86_64-pc-windows-msvc'
}

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$msvc = $null
if (Test-Path $vswhere) {
  $msvc = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
}
Report 'MSVC C++ build tools' ([bool]$msvc) $(if ($msvc) { $msvc } else { 'link.exe not found' }) 'winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"'

$kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
$rc = $null
if (Test-Path $kits) { $rc = Get-ChildItem $kits -Recurse -Filter rc.exe -ErrorAction SilentlyContinue | Where-Object { $_.FullName -match '\\x64\\' } | Select-Object -First 1 }
Report 'Windows SDK (rc.exe)' ([bool]$rc) $(if ($rc) { $rc.FullName } else { 'not found (icons/VERSIONINFO embedding)' }) 'installed with the VCTools workload --includeRecommended'

Write-Section 'Optional: MSI installers (scripts\build-app.ps1 -Msi)'
$dotnet = Test-Command dotnet
Report '.NET SDK' $dotnet '' 'winget install Microsoft.DotNet.SDK.8'
$wix = Test-Command wix
Report 'WiX v5' $wix $(if ($wix) { (wix --version) } else { '' }) 'dotnet tool install --global wix --version 5.0.2'

Write-Section 'Summary'
if ($problems.Count -eq 0) { Write-Host 'Environment ready.' -ForegroundColor Green; exit 0 }
Write-Host "Missing ($($problems.Count)):" -ForegroundColor Yellow
$problems | ForEach-Object { Write-Host "  - $_" }
exit 1
