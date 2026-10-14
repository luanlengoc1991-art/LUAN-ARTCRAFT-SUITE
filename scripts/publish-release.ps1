<#
.SYNOPSIS
  Publish built Windows installers to the owner's GitHub release repos, where ArtCraft Launcher
  (apps/craft-launcher) installs and updates them from.

.DESCRIPTION
  For each app: takes builds/<id>/<version>/windows-x64/ (made by scripts\build-app.ps1 -Msi),
  writes SHA256SUMS.txt (the launcher verifies every download against it) and creates release
  v<version> in config/apps.json releaseRepo (luanlengoc1991-art/<id>) with:
    <stem>-<ver>-windows-x64.msi, <stem>-<ver>-windows-x64-portable.zip, SHA256SUMS.txt
  An existing release with that tag is left untouched (bump the version or delete it yourself).
  Versions with a '-' (e.g. 0.9.0-luan.1) are published as pre-releases.

  Needs: gh (GitHub CLI) logged in to the owner's account.

.EXAMPLE
  scripts\publish-release.ps1                          # every app built by default
  scripts\publish-release.ps1 -Apps pdfcraft,craft-launcher -DryRun
#>
param([string[]] $Apps, [switch] $DryRun)
. "$PSScriptRoot\lib\common.ps1"
# gh writes normal results ("release not found") to stderr; exit codes are checked explicitly below.
$ErrorActionPreference = 'Continue'

$ghDir = 'C:\Program Files\GitHub CLI'
if (-not (Test-Command gh) -and (Test-Path "$ghDir\gh.exe")) { $env:PATH = "$ghDir;$env:PATH" }
if (-not (Test-Command gh)) { throw 'GitHub CLI (gh) not found: winget install GitHub.cli, then gh auth login' }

$Apps = Split-AppList $Apps
if (-not $Apps) { $Apps = @((Get-Manifest).apps | Where-Object { $_.build.buildByDefault } | ForEach-Object { $_.id }) }
$commit = (Invoke-Git rev-parse HEAD).Trim()

$results = @()
foreach ($id in $Apps) {
  $a = Get-App $id
  $ver = Get-AppVersion $a
  $tag = "v$ver"
  $repo = $a.releaseRepo
  $dir = Join-Path $script:RepoRoot "builds\$($a.id)\$ver\windows-x64"
  $files = @(Get-ChildItem $dir -File -ErrorAction SilentlyContinue | Where-Object { $_.Name -match "-$([regex]::Escape($ver))-windows-x64(-portable\.zip|\.msi)$" })
  if ($files.Count -lt 2) {
    $results += [pscustomobject]@{ app = $id; version = $ver; result = "SKIPPED: no .msi + portable .zip in builds\$($a.id)\$ver (run build-app.ps1 -App $id -Msi)" }
    continue
  }
  & gh release view $tag -R $repo *> $null
  if ($LASTEXITCODE -eq 0) { $results += [pscustomobject]@{ app = $id; version = $ver; result = "exists ($repo $tag)" }; continue }

  $sums = Join-Path $dir 'SHA256SUMS.txt'
  $lines = $files | ForEach-Object { "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name }
  [System.IO.File]::WriteAllText($sums, (($lines -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding $false))

  $notes = "$($a.name) $ver for Windows x64.`n`nBuilt from https://github.com/luanlengoc1991-art/LUAN-ARTCRAFT-SUITE/tree/$commit/$($a.path) (upstream $($a.upstream.importedCommit.Substring(0,12))).`nInstall and update with ArtCraft Launcher (https://github.com/luanlengoc1991-art/craft-launcher/releases/latest).`n`nLicense: MIT OR Apache-2.0 (see LICENSE-MIT, LICENSE-APACHE, NOTICE in the source)."
  $assets = @($files.FullName) + $sums
  $ghArgs = @('release', 'create', $tag, '-R', $repo, '--title', "$($a.name) $ver", '--notes', $notes)
  if ($ver -match '-') { $ghArgs += '--prerelease' }
  if ($DryRun) { $results += [pscustomobject]@{ app = $id; version = $ver; result = "dry run: would publish $($files.Name -join ', ') to $repo" }; continue }
  & gh @ghArgs @assets
  if ($LASTEXITCODE -ne 0) { $results += [pscustomobject]@{ app = $id; version = $ver; result = "FAILED (gh exit $LASTEXITCODE)" }; continue }
  # The launcher also ships under fixed names, so .../releases/latest/download/<name> never changes.
  # The launcher's own update check ignores these names (they don't parse as <stem>-<ver>-<os>-<arch>).
  if ($a.id -eq 'craft-launcher') {
    $stable = Join-Path $dir 'stable'
    New-Item -ItemType Directory -Force -Path $stable | Out-Null
    Copy-Item ($files | Where-Object Name -like '*.msi').FullName (Join-Path $stable 'ArtCraft-Launcher-Setup.msi') -Force
    Copy-Item (Join-Path $dir 'artcraft-launcher.exe') (Join-Path $stable 'ArtCraft-Launcher.exe') -Force
    & gh release upload $tag -R $repo (Join-Path $stable 'ArtCraft-Launcher-Setup.msi') (Join-Path $stable 'ArtCraft-Launcher.exe') --clobber
  }
  $results += [pscustomobject]@{ app = $id; version = $ver; result = "published https://github.com/$repo/releases/tag/$tag" }
  Start-Sleep -Seconds 20  # stay clear of GitHub's content-creation rate limit
}
$results | Format-Table -AutoSize -Wrap | Out-String | Write-Host
if ($results | Where-Object { $_.result -like 'FAILED*' }) { exit 1 }
