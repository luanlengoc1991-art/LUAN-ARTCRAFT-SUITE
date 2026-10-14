<#
.SYNOPSIS
  Pull the latest upstream code for ONE app into a new review branch, keeping your edits.

.DESCRIPTION
  Safety rules this script follows:
    * Refuses to run with uncommitted changes (nothing gets stashed, reset or cleaned).
    * Never works on main: creates branch upstream-sync/<id>-<timestamp> and a tag
      pre-sync/<id>-<timestamp> at your current commit as a restore point.
    * Uses `git subtree pull --squash`: a 3-way merge, so your local edits under apps/<id>/
      are kept; upstream never silently overwrites them.
    * On a conflict it STOPS and leaves the merge in progress for you to review. It does not
      pick a side and does not abort. See docs/UPSTREAM-SYNC.md.
    * Never merges into main and never pushes. You do that after testing.

  -DryRun fetches and shows what would change (upstream commits, your local edits, and files
  touched by both, i.e. likely conflicts) without creating a branch or merging.

  -RecordOnly is for after a conflicted sync: once you resolved and committed the merge, it
  writes the upstream commit that is now in apps/<id>/ (read from the latest subtree squash
  commit) and the version into config/apps.json. No fetch, no merge.

.EXAMPLE
  scripts\upstream-update.ps1 -App photocraft -DryRun
  scripts\upstream-update.ps1 -App photocraft
  scripts\upstream-update.ps1 -App photocraft -RecordOnly     # after resolving conflicts
#>
param(
  [Parameter(Mandatory = $true)] [string] $App,
  [switch] $DryRun,
  [switch] $RecordOnly
)
. "$PSScriptRoot\lib\common.ps1"

$a = Get-App $App
$prefix = "apps/$($a.id)"

function Save-SyncRecord([string] $Sha) {
  $fields = @{ importedCommit = $Sha; version = (Get-AppVersion $a) }
  $toml = Get-Content -Raw (Join-Path (Get-AppDir $a) 'Cargo.toml')
  if ($toml -match 'rust-version\s*=\s*"([^"]+)"') { $fields.rustVersion = $Matches[1] }
  Set-ManifestAppFields $a.id $fields
  Invoke-Git add config/apps.json | Out-Null
  Invoke-Git commit -q -m "chore($($a.id)): record upstream commit $($Sha.Substring(0,12)) in config/apps.json" | Out-Null
  return $fields.version
}

if ($RecordOnly) {
  if (Test-Path (Join-Path $script:RepoRoot '.git\MERGE_HEAD')) { throw 'A merge is still in progress. Resolve the conflicts and commit first.' }
  $body = Invoke-Git log -1 --format=%B --grep "^git-subtree-dir: $prefix`$" HEAD
  $split = ($body | Where-Object { $_ -match '^git-subtree-split:\s*([0-9a-f]{40})' } | Select-Object -First 1) -replace '^git-subtree-split:\s*', ''
  if (-not $split) { throw "No subtree squash commit for $prefix found in history." }
  if ($split -eq $a.upstream.importedCommit) { Write-Host "config/apps.json already records $($split.Substring(0,12))."; exit 0 }
  Assert-CleanTree
  $v = Save-SyncRecord $split
  Write-Host "Recorded $($a.id) upstream $($split.Substring(0,12)) (version $v)." -ForegroundColor Green
  exit 0
}

$remote = Initialize-UpstreamRemote $a
$branchRef = "$remote/$($a.upstream.branch)"

Write-Section "Fetching $($a.upstream.repo) ($($a.upstream.branch))"
Invoke-Git fetch --no-tags $remote $a.upstream.branch | Out-Null
$new = (Invoke-Git rev-parse $branchRef).Trim()
$old = $a.upstream.importedCommit
if ($new -eq $old) { Write-Host "$($a.id) is already at upstream $($new.Substring(0,12)). Nothing to do." -ForegroundColor Green; exit 0 }

$upCount = (Invoke-Git rev-list --count "$old..$new").Trim()
Write-Host "Upstream: $($old.Substring(0,12)) -> $($new.Substring(0,12))  ($upCount new upstream commits)"
Invoke-Git log --oneline --no-decorate -15 "$old..$new" | ForEach-Object { Write-Host "  $_" }

# Files upstream changed (relative to the app root) vs files you changed locally.
$upFiles = @(Invoke-Git diff --name-only $old $new)
$localCommits = @(& git -C $script:RepoRoot log --no-merges --format='%H %s' -- "$prefix/" |
    Where-Object { $_ -notmatch "^\S+ Squashed '$prefix/' (content from|changes from)" })
$localFiles = @()
foreach ($c in $localCommits) {
  $sha = ($c -split ' ')[0]
  $localFiles += & git -C $script:RepoRoot diff-tree --no-commit-id --name-only -r $sha -- "$prefix/" |
    ForEach-Object { $_.Substring($prefix.Length + 1) }
}
$localFiles = @($localFiles | Sort-Object -Unique)
$overlap = @($localFiles | Where-Object { $upFiles -contains $_ })

Write-Host ''
Write-Host "Your local commits on $prefix : $($localCommits.Count), files: $($localFiles.Count)"
if ($overlap.Count -gt 0) {
  Write-Host "Files changed by BOTH you and upstream (review these; conflicts are likely):" -ForegroundColor Yellow
  $overlap | ForEach-Object { Write-Host "  $prefix/$_" -ForegroundColor Yellow }
} else { Write-Host 'No file is changed by both sides; the merge should be clean.' -ForegroundColor Green }

if ($DryRun) { Write-Host "`nDry run: no branch created, nothing merged."; exit 0 }

Assert-CleanTree
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$branch = "upstream-sync/$($a.id)-$stamp"
$tag = "pre-sync/$($a.id)-$stamp"
Invoke-Git tag $tag | Out-Null
Invoke-Git switch -c $branch | Out-Null
Write-Section "Restore point: tag $tag   Working branch: $branch"

$msg = "chore($($a.id)): sync upstream storytold/$($a.id)@$($new.Substring(0,12)) via git subtree (squash)"
& git -C $script:RepoRoot subtree pull --prefix=$prefix $remote $a.upstream.branch --squash -m $msg
if ($LASTEXITCODE -ne 0) {
  $conflicts = @(& git -C $script:RepoRoot diff --name-only --diff-filter=U)
  Write-Host ''
  Write-Host 'STOPPED: the upstream merge has conflicts. Nothing was overwritten.' -ForegroundColor Red
  $conflicts | ForEach-Object { Write-Host "  CONFLICT $_" -ForegroundColor Red }
  Write-Host @"

You are on branch $branch with the merge in progress. Next:
  1. Open each conflicted file in Cursor and resolve it (keep your change, upstream's, or combine).
  2. git add <files>; git commit            (completes the merge)
  3. scripts\upstream-update.ps1 -App $($a.id) -RecordOnly   (records $($new.Substring(0,12)) in config\apps.json)
To give up this sync instead: git merge --abort; git switch -   (your main is untouched)
See docs\UPSTREAM-SYNC.md
"@
  exit 2
}

# Record the new upstream commit (and version) in the manifest.
$ver = Save-SyncRecord $new

Write-Host ''
Write-Host "Synced $($a.id) on branch $branch (version now $ver)." -ForegroundColor Green
Write-Host @"
Next (nothing has touched main):
  scripts\test-app.ps1 -App $($a.id)
  scripts\build-app.ps1 -App $($a.id)
  git switch main; git merge --no-ff $branch       # only after the tests pass
"@
