<#
.SYNOPSIS
  Print the version the next release of an app must carry (X.Y.Z, always higher than the last).

.DESCRIPTION
  source = [workspace.package] version in apps/<id>/Cargo.toml (pre-release suffix dropped)
  last   = tag of the latest release in the app's releaseRepo (v prefix dropped), if any
  next   = source when source > last, else last with the patch number + 1

  The launcher compares the installed MSI version (X.Y.Z) with the latest release, so every
  release must be strictly higher. Upstream bumps (0.9.x -> 0.10.0) are followed automatically.
  Needs gh (GH_TOKEN in CI).

.EXAMPLE
  scripts\release-version.ps1 -App gridcraft      # e.g. 0.7.1
#>
param([Parameter(Mandatory = $true)] [string] $App)
. "$PSScriptRoot\lib\common.ps1"
$ErrorActionPreference = 'Continue'
$ghDir = 'C:\Program Files\GitHub CLI'
if (-not (Test-Command gh) -and (Test-Path "$ghDir\gh.exe")) { $env:PATH = "$ghDir;$env:PATH" }

$a = Get-App $App
$source = [version](((Get-AppVersion $a) -split '[-+]')[0])
$tag = & gh release view -R $a.releaseRepo --json tagName --jq .tagName 2>$null
if ($LASTEXITCODE -ne 0 -or -not $tag) { Write-Output $source.ToString(); exit 0 }
$last = [version](($tag.Trim().TrimStart('v') -split '[-+]')[0])
if ($source -gt $last) { Write-Output $source.ToString() }
else { Write-Output ('{0}.{1}.{2}' -f $last.Major, $last.Minor, ($last.Build + 1)) }
