<#
.SYNOPSIS
  Build and install ArtCraft ChatGPT (tools/chatgpt-bridge): chatgpt.com in its own window, signed
  in with your ChatGPT account, that PhotoCraft's AI / MCP Command Center sends image requests to.

.DESCRIPTION
  Builds the Electron app from this repo (npm ci + @electron/packager, needs Node.js) and copies it
  to %LOCALAPPDATA%\Programs\ArtCraft ChatGPT\, where PhotoCraft looks for it, with a Start menu
  shortcut. A running copy is closed first. Run it again to update.

.EXAMPLE
  scripts\install-chatgpt-bridge.ps1
#>
$ErrorActionPreference = 'Stop'
$src = Join-Path $PSScriptRoot '..\tools\chatgpt-bridge' | Resolve-Path
$name = 'ArtCraft ChatGPT'
$dest = Join-Path $env:LOCALAPPDATA "Programs\$name"

if (-not (Get-Command npm -ErrorAction SilentlyContinue)) { throw 'Node.js (npm) is needed to build ArtCraft ChatGPT: https://nodejs.org' }
# VS Code and other Electron hosts set this; it would make Electron run as plain Node.
Remove-Item Env:ELECTRON_RUN_AS_NODE -ErrorAction SilentlyContinue

Push-Location $src
try {
  Write-Host 'npm ci'
  npm ci --no-audit --no-fund
  if ($LASTEXITCODE) { throw "npm ci failed ($LASTEXITCODE)" }
  npm test
  if ($LASTEXITCODE) { throw "tests failed ($LASTEXITCODE)" }
  Write-Host 'Packaging'
  npm run package
  if ($LASTEXITCODE) { throw "packaging failed ($LASTEXITCODE)" }
} finally { Pop-Location }

$built = Join-Path $src "out\$name-win32-x64"
Get-Process -Name $name -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500
if (Test-Path $dest) { Remove-Item $dest -Recurse -Force }
New-Item -ItemType Directory -Force (Split-Path $dest) | Out-Null
Copy-Item $built $dest -Recurse
$exe = Join-Path $dest "$name.exe"

$lnk = Join-Path ([Environment]::GetFolderPath('Programs')) "$name.lnk"
$shell = New-Object -ComObject WScript.Shell
$s = $shell.CreateShortcut($lnk)
$s.TargetPath = $exe
$s.WorkingDirectory = $dest
$s.Description = 'ChatGPT for PhotoCraft (your ChatGPT account)'
$s.Save()

Write-Host "Installed $exe"
Write-Host 'Open it once and sign in to ChatGPT; PhotoCraft starts it by itself after that.'
