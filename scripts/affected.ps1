<#
.SYNOPSIS
  List which apps a set of changes touches, so only those get tested and built.

.DESCRIPTION
  Compares -Base (default: the merge-base with main) against the working tree, including
  uncommitted and untracked files. Each app is a separate Cargo workspace with no path
  dependencies on the others, so a change under apps/<id>/ affects only <id>.
  Changes to scripts/, config/ or .github/ are reported as "suite" changes (they can affect how
  every app is built) but do not change any app's source.

  Prints one app id per line (or JSON with -Json).

.EXAMPLE
  scripts\affected.ps1
  scripts\affected.ps1 -Base HEAD~3 -Json
#>
param([string] $Base, [switch] $Json)
. "$PSScriptRoot\lib\common.ps1"

if (-not $Base) {
  $Base = (& git -C $script:RepoRoot merge-base HEAD main 2>$null)
  if (-not $Base) { $Base = 'HEAD' }
}
$files = @()
$files += & git -C $script:RepoRoot diff --name-only $Base
$files += & git -C $script:RepoRoot ls-files --others --exclude-standard
$files = $files | Where-Object { $_ } | Sort-Object -Unique

$ids = Get-AppIds
$apps = @{}
$suite = @()
foreach ($f in $files) {
  if ($f -match '^apps/([^/]+)/') { if ($ids -contains $Matches[1]) { $apps[$Matches[1]] = $true } }
  elseif ($f -match '^(scripts|config|\.github)/') { $suite += $f }
}
$list = @($apps.Keys | Sort-Object)

# Apps that consume crates of a changed app. They pin that app's crates by git rev on GitHub,
# so they do NOT see your local edit until you add a [patch] or bump the rev (docs/ARCHITECTURE.md).
$consumers = @()
foreach ($c in (Get-Manifest).apps) {
  foreach ($d in $c.dependsOnApps) {
    if ($list -contains $d.app) { $consumers += [pscustomobject]@{ app = $c.id; uses = $d.app; rev = $d.rev } }
  }
}

if ($Json) {
  [pscustomobject]@{ base = $Base; apps = $list; consumers = $consumers; suiteFiles = $suite; files = $files.Count } | ConvertTo-Json -Compress -Depth 4
} else {
  if ($list.Count -eq 0) { Write-Host '(no app changed)' -ForegroundColor DarkGray }
  $list
  foreach ($c in $consumers) {
    Write-Host "note: $($c.app) uses crates of $($c.uses) pinned at git rev $($c.rev); it does not pick up local edits to apps/$($c.uses)" -ForegroundColor Yellow
  }
  if ($suite.Count -gt 0) { Write-Host "suite files changed: $($suite -join ', ')" -ForegroundColor Yellow }
}
