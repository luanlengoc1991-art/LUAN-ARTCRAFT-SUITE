<#
.SYNOPSIS
  Continuous upgrade: sync every app that has new upstream commits, verify, merge into main,
  and push to origin (the owner's GitHub).

.DESCRIPTION
  For each app with an upstream update (one at a time):
    1. scripts\upstream-update.ps1 -App <id>  -> new branch upstream-sync/<id>-<stamp> + restore tag
    2. Conflict  -> STOP the whole run, leave the merge in progress for review (docs/CONFLICTS.md).
    3. Verify    -> scripts\test-app.ps1 -App <id> -Mode check (compile) for apps that are built
                    by default (build.buildByDefault), or for every app with -CheckAll.
                    Failure -> back to main, the branch is kept for review, next app.
    4. Merge     -> git merge --no-ff into main.
  Finally pushes main to origin unless -NoPush. Never force-pushes, never resets.

  Requires: on branch main, clean working tree.

.EXAMPLE
  scripts\upgrade.ps1                       # all apps with updates, then push
  scripts\upgrade.ps1 -Apps photocraft,pdfcraft
  scripts\upgrade.ps1 -CheckAll -NoPush
#>
param(
  [string[]] $Apps,
  [switch] $CheckAll,
  [switch] $NoPush
)
. "$PSScriptRoot\lib\common.ps1"

$Apps = Split-AppList $Apps
if (-not $Apps) { $Apps = Get-AppIds }
$current = (Invoke-Git branch --show-current).Trim()
if ($current -ne 'main') { throw "Run upgrade from main (currently on '$current')." }
Assert-CleanTree
if (-not (Test-Command cargo) -and (Test-Path "$env:USERPROFILE\.cargo\bin\cargo.exe")) { $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH" }
$ps = if (Test-Command pwsh) { 'pwsh' } else { 'powershell' }

$results = @()
foreach ($id in $Apps) {
  $a = Get-App $id
  $line = & git ls-remote $a.upstream.repo "refs/heads/$($a.upstream.branch)" 2>$null
  $remoteSha = if ($line) { ($line -split '\s+')[0] } else { $null }
  if (-not $remoteSha) { $results += [pscustomobject]@{ app = $id; result = 'ERROR: ls-remote failed' }; continue }
  if ($remoteSha -eq $a.upstream.importedCommit) { $results += [pscustomobject]@{ app = $id; result = 'up-to-date' }; continue }

  Write-Section "Upgrading $($a.name)"
  & $ps -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'upstream-update.ps1') -App $id
  $code = $LASTEXITCODE
  $branch = (Invoke-Git branch --show-current).Trim()
  if ($code -eq 2) {
    $results += [pscustomobject]@{ app = $id; result = "CONFLICT on $branch (stopped)" }
    Write-Host "Stopped at $id. Resolve the conflict on $branch (docs\CONFLICTS.md), then re-run upgrade from main." -ForegroundColor Red
    break
  }
  if ($code -ne 0) {
    if ($branch -ne 'main') { Invoke-Git switch main | Out-Null }
    $results += [pscustomobject]@{ app = $id; result = "ERROR: upstream-update exit $code" }; continue
  }

  $check = $CheckAll -or $a.build.buildByDefault
  if ($check) {
    if (-not (Test-Command cargo)) { throw 'cargo not found; cannot verify. Install Rust or use -NoPush and verify later.' }
    & $ps -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'test-app.ps1') -App $id -Mode check
    if ($LASTEXITCODE -ne 0) {
      Invoke-Git switch main | Out-Null
      $results += [pscustomobject]@{ app = $id; result = "CHECK FAILED, not merged (branch $branch kept)" }
      continue
    }
  }
  Invoke-Git switch main | Out-Null
  Invoke-Git merge --no-ff -q -m "Merge $branch" $branch | Out-Null
  # Keep the repo self-contained: copy any git dependency rev the new upstream pins.
  & $ps -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'vendor-git-deps.ps1') -Apps $id
  if ($LASTEXITCODE -eq 0) {
    Invoke-Git add .cargo/config.toml vendor | Out-Null
    $staged = & git -C $script:RepoRoot diff --cached --name-only
    if ($staged) { Invoke-Git commit -q -m "chore($id): vendor git dependencies pinned by the new upstream" | Out-Null }
  } else { Write-Warning "vendor-git-deps failed for $id; run scripts\vendor-git-deps.ps1 -Apps $id" }
  $results += [pscustomobject]@{ app = $id; result = $(if ($check) { 'merged (compile check passed)' } else { 'merged (not compiled: not in build set)' }) }
}

Write-Section 'Upgrade summary'
$results | Format-Table -AutoSize | Out-String | Write-Host

$onMain = ((Invoke-Git branch --show-current).Trim() -eq 'main')
if (-not $NoPush -and $onMain) {
  $remotes = & git -C $script:RepoRoot remote
  if ($remotes -contains 'origin') {
    Write-Section 'Pushing main to origin'
    Invoke-Git push origin main | Out-Null
    Write-Host 'Pushed.' -ForegroundColor Green
  } else { Write-Warning 'No origin remote; nothing pushed.' }
}
