<#
.SYNOPSIS
  Build several (default: all 13) apps one after another; a failure does not stop the others.

.DESCRIPTION
  Calls scripts\build-app.ps1 for each app, captures each log under builds\logs\, and writes
  builds\build-report.json plus a pass/fail table. An app is listed as OK only when build-app.ps1
  exited 0, which means its .exe files exist and passed the PE header check.

.EXAMPLE
  scripts\build-all.ps1
  scripts\build-all.ps1 -Apps pdfcraft,wordcraft -Msi
#>
param(
  [string[]] $Apps,
  [ValidateSet('x64', 'x86', 'arm64')] [string] $Arch = 'x64',
  [switch] $Msi
)
. "$PSScriptRoot\lib\common.ps1"

$Apps = Split-AppList $Apps
if (-not $Apps) { $Apps = Get-AppIds }
$logs = Join-Path $script:RepoRoot 'builds\logs'
New-Item -ItemType Directory -Force -Path $logs | Out-Null
$ps = if (Test-Command pwsh) { 'pwsh' } else { 'powershell' }

$results = @()
foreach ($id in $Apps) {
  $a = Get-App $id
  $log = Join-Path $logs "$($a.id).log"
  $t0 = Get-Date
  Write-Section "[$([array]::IndexOf($Apps, $id) + 1)/$($Apps.Count)] $($a.name)  (log: $log)"
  $buildArgs = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $PSScriptRoot 'build-app.ps1'), '-App', $a.id, '-Arch', $Arch)
  if ($Msi) { $buildArgs += '-Msi' }
  # Merge stderr into the log via cmd so PowerShell 5.1 doesn't turn cargo progress into errors.
  $cmdLine = "`"$ps`" " + (($buildArgs | ForEach-Object { if ($_ -match '\s') { "`"$_`"" } else { $_ } }) -join ' ') + " > `"$log`" 2>&1"
  & cmd.exe /d /c $cmdLine
  $code = $LASTEXITCODE
  $ok = ($code -eq 0)
  $tail = if ($ok) { '' } else { ((Get-Content $log -Tail 15) -join "`n") }
  $results += [pscustomobject]@{ app = $a.id; ok = $ok; exitCode = $code; minutes = [math]::Round(((Get-Date) - $t0).TotalMinutes, 1); log = $log; errorTail = $tail }
  if ($ok) { Write-Host "OK" -ForegroundColor Green } else { Write-Host "FAILED (exit $code)`n$tail" -ForegroundColor Red }
}

$report = Join-Path $script:RepoRoot 'builds\build-report.json'
[pscustomobject]@{ arch = $Arch; msi = [bool]$Msi; finishedAt = (Get-Date).ToUniversalTime().ToString('o'); results = $results } |
  ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 $report

Write-Section 'Build summary'
$results | Format-Table app, ok, exitCode, minutes -AutoSize | Out-String | Write-Host
$failed = @($results | Where-Object { -not $_.ok })
Write-Host "$($results.Count - $failed.Count)/$($results.Count) succeeded. Report: $report"
if ($failed.Count -gt 0) { exit 1 }
