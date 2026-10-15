<#
.SYNOPSIS
  Install ArtCraft MCP (one MCP server for every installed ArtCraft app) and register it with
  Claude Code and Cursor.

.DESCRIPTION
  Downloads artcraft-mcp.exe from the fixed link
    https://github.com/luanlengoc1991-art/LUAN-ARTCRAFT-SUITE/releases/latest/download/ArtCraft-MCP.exe
  to %LOCALAPPDATA%\Programs\ArtCraft MCP\, then registers it as the MCP server "artcraft":
    Claude Code  user scope (all projects), via `claude mcp add`
    Cursor       %USERPROFILE%\.cursor\mcp.json, with --compact (only the artcraft_* tools, since
                 Cursor caps how many tools it sends; artcraft_tools + artcraft_call reach every app)
  Run it again to update. Standalone: works without a clone of the repo
    irm https://raw.githubusercontent.com/luanlengoc1991-art/LUAN-ARTCRAFT-SUITE/main/scripts/install-mcp.ps1 | iex

  Apps installed by ArtCraft Launcher (Program Files) are found automatically. Folders of existing
  per-app MCP entries (e.g. D:\Apps\VectorCraft) are added to the search list. -RemoveAppEntries
  drops those per-app entries so no tool is listed twice.

.EXAMPLE
  scripts\install-mcp.ps1
  scripts\install-mcp.ps1 -SearchDir D:\Apps -Folder D:\ArtCraftWork -RemoveAppEntries
  scripts\install-mcp.ps1 -Exe tools\artcraft-mcp\target\release\artcraft-mcp.exe   # local build
#>
param(
  [string] $Exe,
  [string[]] $SearchDir,
  [string] $Folder,
  [switch] $RemoveAppEntries,
  [switch] $NoClaude,
  [switch] $NoCursor
)
$ErrorActionPreference = 'Stop'
$url = 'https://github.com/luanlengoc1991-art/LUAN-ARTCRAFT-SUITE/releases/latest/download/ArtCraft-MCP.exe'
$appIds = 'photocraft', 'vectorcraft', 'filmcraft', 'lightcraft', 'pdfcraft', 'effectcraft', 'designcraft',
  'figcraft', 'wordcraft', 'soundcraft', 'deckcraft', 'cadcraft', 'gridcraft'
$utf8 = New-Object System.Text.UTF8Encoding $false

function Read-Json([string] $Path) {
  if (-not (Test-Path $Path)) { return [pscustomobject]@{} }
  $t = [IO.File]::ReadAllText($Path).TrimStart([char]0xFEFF)
  if (-not $t.Trim()) { return [pscustomobject]@{} }
  return $t | ConvertFrom-Json
}
function Write-Json([string] $Path, $Value) {
  New-Item -ItemType Directory -Force -Path (Split-Path $Path) | Out-Null
  [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 20) + "`n", $utf8)
}
function Set-Prop($Obj, [string] $Name, $Value) { $Obj | Add-Member -NotePropertyName $Name -NotePropertyValue $Value -Force }

# --- 1. The exe -------------------------------------------------------------------------------
$dir = Join-Path $env:LOCALAPPDATA 'Programs\ArtCraft MCP'
$target = Join-Path $dir 'artcraft-mcp.exe'
New-Item -ItemType Directory -Force $dir | Out-Null
$tmp = "$target.download"
if ($Exe) { Copy-Item (Resolve-Path $Exe) $tmp -Force }
else {
  Write-Host "Downloading $url"
  [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
  Invoke-WebRequest $url -OutFile $tmp -UseBasicParsing
}
# A running exe can't be overwritten but can be renamed: Claude Code / Cursor may be using it.
if (Test-Path $target) {
  Remove-Item "$target.old" -Force -ErrorAction SilentlyContinue
  Move-Item $target "$target.old" -Force
}
Move-Item $tmp $target -Force
$version = & $target --version
Write-Host "Installed $version -> $target"

# --- 2. Existing per-app entries: learn where apps live, optionally remove them ----------------
$cursorPath = Join-Path $env:USERPROFILE '.cursor\mcp.json'
$claudeJson = Join-Path $env:USERPROFILE '.claude.json'
$found = @()
foreach ($p in @($cursorPath, $claudeJson)) {
  $j = Read-Json $p
  if (-not $j.mcpServers) { continue }
  foreach ($s in $j.mcpServers.PSObject.Properties) {
    if ($appIds -notcontains $s.Name -or -not $s.Value.command) { continue }
    $found += $s.Name
    $appDir = Split-Path $s.Value.command
    if ($appDir -and (Test-Path $appDir)) { $SearchDir += (Split-Path $appDir) }
  }
}
$found = $found | Sort-Object -Unique

# --- 3. Gateway config ------------------------------------------------------------------------
$cfgPath = Join-Path $env:LOCALAPPDATA 'ArtCraft\artcraft-mcp\config.json'
$cfg = Read-Json $cfgPath
$dirs = @($cfg.searchDirs) + @($SearchDir) | Where-Object { $_ } | ForEach-Object { $_.TrimEnd('\') } | Sort-Object -Unique
$pf = @($env:ProgramFiles, ${env:ProgramFiles(x86)}) | Where-Object { $_ }
$dirs = @($dirs | Where-Object { $pf -notcontains $_ })
if ($dirs.Count) { Set-Prop $cfg 'searchDirs' $dirs }
if ($Folder) { Set-Prop $cfg 'folder' ((Resolve-Path $Folder).Path) }
Write-Json $cfgPath $cfg
Write-Host "Config: $cfgPath"

# --- 4. Claude Code ----------------------------------------------------------------------------
function Find-Claude {
  $c = Get-Command claude -ErrorAction SilentlyContinue
  if ($c) { return $c.Source }
  $local = Join-Path $env:USERPROFILE '.local\bin\claude.exe'
  if (Test-Path $local) { return $local }
  $ext = foreach ($root in @('.vscode', '.cursor', '.vscode-insiders', '.windsurf')) {
    Get-ChildItem (Join-Path $env:USERPROFILE "$root\extensions") -Directory -Filter 'anthropic.claude-code-*' -ErrorAction SilentlyContinue
  }
  $exe = $ext | Sort-Object LastWriteTime -Descending | ForEach-Object { Join-Path $_.FullName 'resources\native-binary\claude.exe' } |
    Where-Object { Test-Path $_ } | Select-Object -First 1
  return $exe
}
# Windows PowerShell turns a native command's stderr into an error under 'Stop'; claude prints
# "no server named ..." there, which is expected on a first install.
function Invoke-Claude([string[]] $CliArgs) {
  $prev = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  $out = & $claude @CliArgs 2>&1
  $code = $LASTEXITCODE
  $ErrorActionPreference = $prev
  return [pscustomobject]@{ Code = $code; Output = ($out | Out-String).Trim() }
}
if (-not $NoClaude) {
  $claude = Find-Claude
  if ($claude) {
    Invoke-Claude @('mcp', 'remove', 'artcraft', '-s', 'user') | Out-Null
    $r = Invoke-Claude @('mcp', 'add', 'artcraft', '-s', 'user', '--', $target)
    if ($r.Code -ne 0) { throw "claude mcp add failed ($($r.Code)): $($r.Output)" }
    Write-Host "Claude Code: registered 'artcraft' (user scope) with $claude"
    if ($RemoveAppEntries) {
      foreach ($id in $found) { Invoke-Claude @('mcp', 'remove', $id, '-s', 'user') | Out-Null }
    }
  } else {
    Write-Warning "Claude Code CLI not found. Run: claude mcp add artcraft -s user -- `"$target`""
  }
}

# --- 5. Cursor -----------------------------------------------------------------------------------
if (-not $NoCursor) {
  $j = Read-Json $cursorPath
  if (-not $j.mcpServers) { Set-Prop $j 'mcpServers' ([pscustomobject]@{}) }
  Set-Prop $j.mcpServers 'artcraft' ([pscustomobject]@{ command = $target; args = @('--compact') })
  if ($RemoveAppEntries) { foreach ($id in $found) { $j.mcpServers.PSObject.Properties.Remove($id) } }
  Write-Json $cursorPath $j
  Write-Host "Cursor: registered 'artcraft' in $cursorPath"
}

# --- 6. Report -----------------------------------------------------------------------------------
Write-Host ''
& $target --list
Write-Host ''
if ($found -and -not $RemoveAppEntries) {
  Write-Host "Per-app entries still registered: $($found -join ', '). Their tools also come through 'artcraft' now;"
  Write-Host 'run again with -RemoveAppEntries to drop them.'
}
Write-Host 'Reload Claude Code (/mcp, or reload the window) and Cursor (Settings > MCP) to load "artcraft".'
