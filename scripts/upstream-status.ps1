<#
.SYNOPSIS
  Show, for every app, whether upstream has new commits and whether you have local edits.
  Read-only: does not fetch into branches, merge or change any file.

.DESCRIPTION
  upstream head  : `git ls-remote` of the app's upstream branch (network, no fetch needed)
  imported       : upstream.importedCommit recorded in config/apps.json
  local commits  : non-merge commits in this monorepo that touched apps/<id>/ and are not
                   subtree imports, i.e. your own customizations that a sync must preserve.

.EXAMPLE
  scripts\upstream-status.ps1
  scripts\upstream-status.ps1 -Json > status.json
#>
param([string[]] $Apps, [switch] $Json)
. "$PSScriptRoot\lib\common.ps1"

$Apps = Split-AppList $Apps
if (-not $Apps) { $Apps = Get-AppIds }
$rows = @()
foreach ($id in $Apps) {
  $a = Get-App $id
  $ref = "refs/heads/$($a.upstream.branch)"
  $remote = $null; $err = $null
  try {
    $line = & git ls-remote $a.upstream.repo $ref 2>$null
    if ($LASTEXITCODE -eq 0 -and $line) { $remote = ($line -split '\s+')[0] } else { $err = 'ls-remote failed' }
  } catch { $err = $_.Exception.Message }

  $local = @(& git -C $script:RepoRoot log --no-merges --format='%h %s' -- "apps/$($a.id)/" |
      Where-Object { $_ -notmatch "^\S+ Squashed 'apps/$($a.id)/' (content from|changes from)" })

  $state = if ($err) { 'ERROR' } elseif ($remote -eq $a.upstream.importedCommit) { 'up-to-date' } else { 'UPDATE AVAILABLE' }
  $rows += [pscustomobject]@{
    app = $a.id; state = $state
    imported = $a.upstream.importedCommit.Substring(0, 10)
    upstream = if ($remote) { $remote.Substring(0, 10) } else { '-' }
    upstreamFull = $remote
    localCommits = $local.Count
    error = $err
  }
}

if ($Json) { $rows | ConvertTo-Json -Depth 4; exit 0 }
$rows | Format-Table app, state, imported, upstream, localCommits -AutoSize | Out-String | Write-Host
$n = @($rows | Where-Object { $_.state -eq 'UPDATE AVAILABLE' }).Count
Write-Host "$n app(s) have upstream updates. Update one with: scripts\upstream-update.ps1 -App <id>"
