<#
.SYNOPSIS
  Register the MCP server of every installed ArtCraft app with Claude Code and Cursor.

.DESCRIPTION
  Each app's MSI installs <id>-cli.exe next to the app (Program Files\<App>\). `<id>-cli mcp` is
  that app's MCP server (JSON-RPC over stdio). Because updates replace the files in the same
  folder, the registration keeps working after the launcher updates an app: the MCP server is
  always the installed version.

  Writes (merging, other servers are kept):
    <repo>\.mcp.json                 Claude Code, project scope (approve once when Claude asks)
    %USERPROFILE%\.cursor\mcp.json   Cursor, all projects
  Apps that are not installed are left out; run again after installing more apps.
  -Folder confines file access of the apps that support it (--automation-read/write-root).

.EXAMPLE
  scripts\register-mcp.ps1
  scripts\register-mcp.ps1 -Folder D:\ArtCraftWork
#>
param([string] $Folder)
. "$PSScriptRoot\lib\common.ps1"

# Flags each app's README documents for confining file access.
$rootFlags = @{
  photocraft = 'automation'; vectorcraft = 'automation'; wordcraft = 'automation'; soundcraft = 'automation'
  pdfcraft = 'root'
}

$servers = [ordered]@{}
$rows = @()
foreach ($a in (Get-Manifest).apps) {
  if (-not $a.installFolder -or $a.installFolder -notlike '%ProgramFiles%*') { continue }
  $dir = $a.installFolder -replace '^%ProgramFiles%', $env:ProgramFiles
  $exe = $null
  if (Test-Path $dir) { $exe = Get-ChildItem $dir -Recurse -Filter "$($a.id)-cli.exe" -ErrorAction SilentlyContinue | Select-Object -First 1 }
  if (-not $exe) { $rows += [pscustomobject]@{ app = $a.id; mcp = 'not installed'; path = '' }; continue }
  $mcpArgs = @('mcp')
  if ($Folder) {
    switch ($rootFlags[$a.id]) {
      'automation' { $mcpArgs += @('--automation-read-root', $Folder, '--automation-write-root', $Folder) }
      'root' { $mcpArgs += @('--root', $Folder) }
    }
  }
  $servers[$a.id] = [ordered]@{ command = $exe.FullName; args = $mcpArgs }
  $rows += [pscustomobject]@{ app = $a.id; mcp = 'registered'; path = $exe.FullName }
}

function Merge-McpFile([string] $Path) {
  $existing = [ordered]@{}
  if (Test-Path $Path) {
    $j = Get-Content -Raw $Path | ConvertFrom-Json
    if ($j.mcpServers) { foreach ($p in $j.mcpServers.PSObject.Properties) { $existing[$p.Name] = $p.Value } }
  }
  foreach ($k in $servers.Keys) { $existing[$k] = $servers[$k] }
  New-Item -ItemType Directory -Force -Path (Split-Path $Path) | Out-Null
  $json = [ordered]@{ mcpServers = $existing } | ConvertTo-Json -Depth 6
  [System.IO.File]::WriteAllText($Path, $json + "`n", (New-Object System.Text.UTF8Encoding $false))
  Write-Host "wrote $Path ($($existing.Count) servers)"
}

$rows | Format-Table -AutoSize | Out-String | Write-Host
if ($servers.Count -eq 0) { Write-Warning 'No installed app found. Install apps with ArtCraft Launcher first.'; exit 1 }
Merge-McpFile (Join-Path $script:RepoRoot '.mcp.json')
Merge-McpFile (Join-Path $env:USERPROFILE '.cursor\mcp.json')
Write-Host 'Restart Cursor / Claude Code to load the servers.'
