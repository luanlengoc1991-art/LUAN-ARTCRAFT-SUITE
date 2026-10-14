<#
.SYNOPSIS
  Point rust-analyzer (Cursor / VS Code) at the app(s) you are working on.

.DESCRIPTION
  Rewrites "rust-analyzer.linkedProjects" in .vscode/settings.json to apps/<id>/Cargo.toml for the
  given apps. -All links all 13 (needs a lot of RAM); -None clears it. Then reload the window
  (Ctrl+Shift+P > "Developer: Reload Window") or run "rust-analyzer: Restart server".

.EXAMPLE
  scripts\focus-app.ps1 -App photocraft
  scripts\focus-app.ps1 -App effectcraft,filmcraft
  scripts\focus-app.ps1 -None
#>
param([string[]] $App, [switch] $All, [switch] $None)
. "$PSScriptRoot\lib\common.ps1"

$ids = if ($All) { Get-AppIds } elseif ($None) { @() } elseif ($App) { Split-AppList $App | ForEach-Object { (Get-App $_).id } } else { throw 'Give -App <id>[,<id>], -All or -None' }
$paths = @($ids | ForEach-Object { "apps/$_/Cargo.toml" })
$value = if ($paths.Count -eq 0) { '[]' } else { "[`n    " + (($paths | ForEach-Object { "`"$_`"" }) -join ",`n    ") + "`n  ]" }

$file = Join-Path $script:RepoRoot '.vscode\settings.json'
$text = [System.IO.File]::ReadAllText($file)
$re = '"rust-analyzer\.linkedProjects"\s*:\s*\[[^\]]*\]'
if ($text -notmatch $re) { throw "rust-analyzer.linkedProjects not found in $file" }
$text = [regex]::Replace($text, $re, "`"rust-analyzer.linkedProjects`": $value")
[System.IO.File]::WriteAllText($file, $text, (New-Object System.Text.UTF8Encoding $false))
Write-Host "rust-analyzer now loads: $(if ($ids.Count) { $ids -join ', ' } else { '(nothing)' }). Reload the Cursor window."
