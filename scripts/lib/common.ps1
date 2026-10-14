# Shared helpers for LUAN-ARTCRAFT-SUITE scripts. Dot-source: . "$PSScriptRoot\lib\common.ps1"
# Compatible with Windows PowerShell 5.1 and PowerShell 7+.

$ErrorActionPreference = 'Stop'
$script:RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$script:ManifestPath = Join-Path $script:RepoRoot 'config\apps.json'

function Get-Manifest {
  return (Get-Content -Raw -Encoding UTF8 $script:ManifestPath | ConvertFrom-Json)
}

function Save-Manifest($Manifest) {
  $json = $Manifest | ConvertTo-Json -Depth 20
  # UTF-8 without BOM, LF line endings, trailing newline.
  $json = ($json -replace "`r`n", "`n") + "`n"
  [System.IO.File]::WriteAllText($script:ManifestPath, $json, (New-Object System.Text.UTF8Encoding $false))
}

function Get-AppIds { return @((Get-Manifest).apps | ForEach-Object { $_.id }) }

function Get-App([string] $Id) {
  $app = (Get-Manifest).apps | Where-Object { $_.id -eq $Id.ToLower() }
  if (-not $app) { throw "Unknown app '$Id'. Valid: $((Get-AppIds) -join ', ')" }
  return $app
}

function Get-AppDir($App) { return (Join-Path $script:RepoRoot ($App.path -replace '/', '\')) }

# Read [workspace.package] version from the app's root Cargo.toml (same rule as upstream package.ps1).
function Get-AppVersion($App) {
  $inPkg = $false
  foreach ($line in Get-Content (Join-Path (Get-AppDir $App) 'Cargo.toml')) {
    if ($line -match '^\s*\[') { $inPkg = ($line.Trim() -eq '[workspace.package]'); continue }
    if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { return $Matches[1] }
  }
  throw "could not read [workspace.package] version for $($App.id)"
}

function Invoke-Git {
  # Runs git in the repo root; throws on non-zero exit. Returns stdout lines.
  $out = & git -C $script:RepoRoot @args
  if ($LASTEXITCODE -ne 0) { throw "git $($args -join ' ') failed (exit $LASTEXITCODE)" }
  return $out
}

function Test-Command([string] $Name) { return [bool](Get-Command $Name -ErrorAction SilentlyContinue) }

# Ensure the upstream-<id> remote exists (fresh clones of the monorepo don't carry remotes).
function Initialize-UpstreamRemote($App) {
  $name = "upstream-$($App.id)"
  $existing = & git -C $script:RepoRoot remote
  if ($existing -notcontains $name) {
    Invoke-Git remote add $name $App.upstream.repo | Out-Null
    Write-Host "added remote $name -> $($App.upstream.repo)"
  }
  return $name
}

function Assert-CleanTree {
  $status = Invoke-Git status --porcelain
  if ($status) {
    throw "Working tree has uncommitted changes. Commit or stash them first (nothing was touched):`n$($status -join "`n")"
  }
}

function Write-Section([string] $Text) { Write-Host ''; Write-Host "==== $Text" -ForegroundColor Cyan }

# Accept "a,b" as well as @('a','b'): `powershell -File x.ps1 -Apps a,b` passes one string.
function Split-AppList([string[]] $List) {
  return @($List | ForEach-Object { $_ -split '[,\s]+' } | Where-Object { $_ })
}
