<#
.SYNOPSIS
  Check, lint or test one app inside its own Cargo workspace.

.DESCRIPTION
  -Mode check : cargo check --locked --workspace --all-targets   (fast, compile only)
  -Mode test  : cargo test  --locked                             (the app's default-members, as upstream CI)
  -Mode full  : cargo fmt --check, cargo clippy --locked -- -D warnings, cargo test --locked --workspace
  Extra arguments after -- are passed to cargo (e.g. -p photocraft-engine).

.EXAMPLE
  scripts\test-app.ps1 -App gridcraft
  scripts\test-app.ps1 -App photocraft -Mode check
  scripts\test-app.ps1 -App pdfcraft -Mode test -- -p pdfcraft-engine
#>
param(
  [Parameter(Mandatory = $true)] [string] $App,
  [ValidateSet('check', 'test', 'full')] [string] $Mode = 'test',
  [Parameter(ValueFromRemainingArguments = $true)] [string[]] $Rest
)
. "$PSScriptRoot\lib\common.ps1"

$a = Get-App $App
if (-not (Test-Command cargo) -and (Test-Path "$env:USERPROFILE\.cargo\bin\cargo.exe")) { $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH" }
if (-not (Test-Command cargo)) { throw 'cargo not found. Run scripts\check-env.ps1.' }
$extra = @($Rest | Where-Object { $_ -ne '--' })

function Run([string[]] $CargoArgs) {
  Write-Host "> cargo $($CargoArgs -join ' ')" -ForegroundColor Cyan
  & cargo @CargoArgs
  if ($LASTEXITCODE -ne 0) { throw "cargo $($CargoArgs[0]) failed for $($a.id) (exit $LASTEXITCODE)" }
}

Write-Section "$($a.name): $Mode"
Push-Location (Get-AppDir $a)
try {
  switch ($Mode) {
    'check' { Run (@('check', '--locked', '--workspace', '--all-targets') + $extra) }
    'test' { Run (@('test', '--locked') + $extra) }
    'full' {
      Run @('fmt', '--all', '--', '--check')
      Run (@('clippy', '--locked', '--workspace', '--all-targets') + $extra + @('--', '-D', 'warnings'))
      Run (@('test', '--locked', '--workspace') + $extra)
    }
  }
} finally { Pop-Location }
Write-Host "$($a.id): $Mode passed" -ForegroundColor Green
