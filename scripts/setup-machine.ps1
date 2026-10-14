<#
.SYNOPSIS
  Prepare a new Windows 11 x64 machine to edit, build, package and publish LUAN-ARTCRAFT-SUITE.

.DESCRIPTION
  Installs with winget (skips what is already there): Git, Rust (rustup, MSVC stable), Visual
  Studio 2022 Build Tools with the C++ workload + Windows SDK (needs admin / UAC, ~7 GB), .NET 8
  SDK + WiX 5.0.2 (MSI installers), GitHub CLI. Sets git core.longpaths. Then runs check-env.ps1.

  First time on a new machine (PowerShell):
    winget install --id Git.Git -e
    git clone -c core.longpaths=true https://github.com/luanlengoc1991-art/LUAN-ARTCRAFT-SUITE.git
    cd LUAN-ARTCRAFT-SUITE
    powershell -ExecutionPolicy Bypass -File scripts\setup-machine.ps1
    gh auth login          # only to publish releases / create repos

.EXAMPLE
  scripts\setup-machine.ps1
  scripts\setup-machine.ps1 -SkipMsiTools
#>
param([switch] $SkipMsiTools)
$ErrorActionPreference = 'Continue'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path

function Install-Winget([string] $Id, [string] $Override) {
  $listed = winget list --id $Id -e --accept-source-agreements 2>$null | Select-String -SimpleMatch $Id
  if ($listed) { Write-Host "[have] $Id" -ForegroundColor Green; return }
  Write-Host "[install] $Id" -ForegroundColor Cyan
  $wargs = @('install', '--id', $Id, '-e', '--accept-source-agreements', '--accept-package-agreements')
  if ($Override) { $wargs += @('--override', $Override) }
  & winget @wargs
}

Install-Winget 'Git.Git'
Install-Winget 'Rustlang.Rustup'
Install-Winget 'Microsoft.VisualStudio.2022.BuildTools' '--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended'
Install-Winget 'GitHub.cli'
if (-not $SkipMsiTools) { Install-Winget 'Microsoft.DotNet.SDK.8' }

$env:PATH = "$env:USERPROFILE\.cargo\bin;C:\Program Files\Git\cmd;C:\Program Files\dotnet;$env:USERPROFILE\.dotnet\tools;C:\Program Files\GitHub CLI;$env:PATH"
if (Get-Command rustup -ErrorAction SilentlyContinue) {
  rustup default stable-x86_64-pc-windows-msvc
  rustup update stable
}
if (-not $SkipMsiTools -and (Get-Command dotnet -ErrorAction SilentlyContinue)) {
  if (-not (Get-Command wix -ErrorAction SilentlyContinue)) { dotnet tool install --global wix --version 5.0.2 }
  wix extension add -g WixToolset.UI.wixext/5.0.2 WixToolset.Util.wixext/5.0.2
}
git -C $root config core.longpaths true
git config --global core.longpaths true

Write-Host ''
Write-Host 'Open a NEW PowerShell window (PATH changes), then check:' -ForegroundColor Yellow
& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'check-env.ps1')
