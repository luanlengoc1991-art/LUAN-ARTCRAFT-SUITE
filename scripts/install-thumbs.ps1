<#
.SYNOPSIS
  Install ArtCraft Thumbnails: Windows Explorer shows what is inside .pcraft (PhotoCraft) and
  .vectorcraft / .drawcraft (VectorCraft) files, with the app's icon in the corner.

.DESCRIPTION
  Downloads artcraft-thumbs.dll from the fixed link
    https://github.com/luanlengoc1991-art/LUAN-ARTCRAFT-SUITE/releases/download/artcraft-thumbs/ArtCraft-Thumbs.dll
  to %LOCALAPPDATA%\Programs\ArtCraft Thumbnails\ and registers it for the current user (HKCU, no
  admin) as the thumbnail handler of those file types. Run it again to update; -Uninstall removes
  it. Standalone: works without a clone of the repo
    irm https://raw.githubusercontent.com/luanlengoc1991-art/LUAN-ARTCRAFT-SUITE/main/scripts/install-thumbs.ps1 | iex

  The badge is the icon of the installed app (%ProgramFiles%\PhotoCraft\photocraft.exe, ...).
  Apps installed elsewhere: pass their folder with -SearchDir. Without the app the thumbnail
  shows without a badge.

  The thumbnail is the preview each app embeds when it saves. PhotoCraft always embeds one;
  VectorCraft does from the version that turned it on by default (older files: open and save).

.EXAMPLE
  scripts\install-thumbs.ps1
  scripts\install-thumbs.ps1 -SearchDir D:\Apps
  scripts\install-thumbs.ps1 -Dll tools\artcraft-thumbs\target\release\artcraft_thumbs.dll   # local build
  scripts\install-thumbs.ps1 -Uninstall
#>
param(
  [string] $Dll,
  [string[]] $SearchDir,
  [switch] $Uninstall
)
$ErrorActionPreference = 'Stop'
$url = 'https://github.com/luanlengoc1991-art/LUAN-ARTCRAFT-SUITE/releases/download/artcraft-thumbs/ArtCraft-Thumbs.dll'
# Must match CLSID in tools/artcraft-thumbs/src/com.rs.
$clsid = '{DBC0383E-886F-4B5F-B407-AFEA2F1DF2E0}'
$thumbHandler = '{e357fccd-a995-4576-b01f-234630154e96}'  # IThumbnailProvider
$extensions = '.pcraft', '.vectorcraft', '.drawcraft'
$apps = @{ photocraft = 'PhotoCraft'; vectorcraft = 'VectorCraft' }
$classes = 'HKCU:\Software\Classes'
$appsKey = 'HKCU:\Software\ArtCraft\Thumbnails'
$approved = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Shell Extensions\Approved'
$dir = Join-Path $env:LOCALAPPDATA 'Programs\ArtCraft Thumbnails'
$target = Join-Path $dir 'artcraft-thumbs.dll'

function Set-Default([string] $Key, [string] $Value) {
  New-Item -Path $Key -Force | Out-Null
  Set-ItemProperty -LiteralPath $Key -Name '(default)' -Value $Value
}

# Tell Explorer the handlers changed, so it asks for thumbnails again.
function Send-AssocChanged {
  if (-not ('ArtCraftThumbs.Shell' -as [type])) {
    Add-Type -Namespace ArtCraftThumbs -Name Shell -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("shell32.dll")]
public static extern void SHChangeNotify(int eventId, uint flags, System.IntPtr item1, System.IntPtr item2);
'@
  }
  [ArtCraftThumbs.Shell]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
}

if ($Uninstall) {
  foreach ($ext in $extensions) {
    $k = "$classes\$ext\ShellEx\$thumbHandler"
    if ((Test-Path $k) -and (Get-ItemProperty -LiteralPath $k).'(default)' -eq $clsid) { Remove-Item $k -Force }
  }
  if (Test-Path "$classes\CLSID\$clsid") { Remove-Item "$classes\CLSID\$clsid" -Recurse -Force }
  if (Test-Path $approved) { Remove-ItemProperty -LiteralPath $approved -Name $clsid -ErrorAction SilentlyContinue }
  if (Test-Path $appsKey) { Remove-Item $appsKey -Recurse -Force }
  Send-AssocChanged
  # Explorer may still hold the DLL; it goes once Explorer lets go (or at the next install).
  Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
  Write-Host 'ArtCraft Thumbnails removed.'
  return
}

# --- 1. The DLL ---------------------------------------------------------------------------------
New-Item -ItemType Directory -Force $dir | Out-Null
$tmp = "$target.download"
if ($Dll) { Copy-Item (Resolve-Path $Dll) $tmp -Force }
else {
  Write-Host "Downloading $url"
  [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
  Invoke-WebRequest $url -OutFile $tmp -UseBasicParsing
}
# A loaded DLL can't be overwritten but can be renamed: Explorer may be using it.
if (Test-Path $target) {
  Get-ChildItem $dir -Filter 'artcraft-thumbs.dll.old*' | Remove-Item -Force -ErrorAction SilentlyContinue
  $old = "$target.old"
  if (Test-Path $old) { $old = "$target.old-$([DateTime]::Now.Ticks)" }
  Move-Item $target $old -Force
}
Move-Item $tmp $target -Force
Write-Host "Installed -> $target"

# --- 2. Register it for this user ---------------------------------------------------------------
Set-Default "$classes\CLSID\$clsid" 'ArtCraft Thumbnails'
Set-Default "$classes\CLSID\$clsid\InprocServer32" $target
Set-ItemProperty -LiteralPath "$classes\CLSID\$clsid\InprocServer32" -Name ThreadingModel -Value Apartment
New-Item -Path $approved -Force | Out-Null
Set-ItemProperty -LiteralPath $approved -Name $clsid -Value 'ArtCraft Thumbnails'
foreach ($ext in $extensions) { Set-Default "$classes\$ext\ShellEx\$thumbHandler" $clsid }

# --- 3. Where the apps are (for the badge) ------------------------------------------------------
New-Item -Path $appsKey -Force | Out-Null
foreach ($id in $apps.Keys) {
  $candidates = @(
    (Join-Path $env:ProgramFiles "$($apps[$id])\$id.exe"),
    (Join-Path $env:LOCALAPPDATA "Programs\$($apps[$id])\$id.exe")
  )
  foreach ($d in @($SearchDir)) {
    if ($d) { $candidates += (Join-Path $d "$id.exe"), (Join-Path $d "$($apps[$id])\$id.exe") }
  }
  $exe = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
  if ($exe) {
    Set-ItemProperty -LiteralPath $appsKey -Name $id -Value $exe
    Write-Host "  $($apps[$id]): $exe"
  } else {
    Remove-ItemProperty -LiteralPath $appsKey -Name $id -ErrorAction SilentlyContinue
    Write-Host "  $($apps[$id]): not found (thumbnails show without its badge)"
  }
}

Send-AssocChanged
Write-Host "Registered for $($extensions -join ', '). Explorer shows the thumbnails in Medium icons or larger."
Write-Host 'Old thumbnails cached by Explorer: Disk Cleanup > Thumbnails clears them.'
