#!/usr/bin/env pwsh
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string] $Setup,
  [Parameter(Mandatory = $true)][string] $Payload,
  [Parameter(Mandatory = $true)][string] $Version
)

$ErrorActionPreference = 'Stop'
$script:failures = @()

function Assert([bool] $Condition, [string] $Message) {
  if ($Condition) {
    Write-Host "ok   $Message"
  }
  else {
    $script:failures += $Message
    Write-Host "FAIL $Message"
  }
}

function Invoke-Exe([string] $Path, [string] $Arguments, [int] $TimeoutSeconds = 180) {
  $process = Start-Process -FilePath $Path -ArgumentList $Arguments -PassThru
  $null = $process.Handle
  if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
    Stop-Process -Id $process.Id -Force
    return 'timeout'
  }
  return $process.ExitCode
}

function Get-RelativeFiles([string] $Root) {
  $full = (Resolve-Path -LiteralPath $Root).Path.TrimEnd('\', '/')
  return @(Get-ChildItem -LiteralPath $full -File -Recurse -Force | ForEach-Object { $_.FullName.Substring($full.Length + 1) } | Sort-Object)
}

function Get-RunValue {
  $values = Get-ItemProperty -Path $runPath -ErrorAction SilentlyContinue
  if ($values) { return $values.Heartwire }
  return $null
}

function Set-RunValue([string] $Exe) {
  New-ItemProperty -Path $runPath -Name 'Heartwire' -Value ('"' + $Exe + '" --minimized') -PropertyType String -Force | Out-Null
}

function Add-RuntimeFiles([string] $Dir) {
  foreach ($name in $runtimeFiles) {
    New-Item -ItemType File -Force -Path (Join-Path $Dir $name) | Out-Null
  }
}

function Write-Text([string] $Path, [string] $Text) {
  New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Path) | Out-Null
  [System.IO.File]::WriteAllText($Path, $Text, (New-Object System.Text.UTF8Encoding($false)))
}

function Invoke-Uninstall([string] $Dir) {
  $code = Invoke-Exe (Join-Path $Dir 'Uninstall.exe') "/S _?=$Dir"
  Assert ("$code" -eq '0') "uninstaller exit code 0 (got $code)"
  Assert (-not (Test-Path -Path $arpPath)) "uninstall entry removed"
  Assert (-not (Test-Path -LiteralPath $shortcut)) "start menu shortcut removed"
  $left = @()
  if (Test-Path -LiteralPath $Dir) {
    $left = @(Get-ChildItem -LiteralPath $Dir -Recurse -Force | Where-Object { $_.Name -ne 'Uninstall.exe' })
  }
  Assert ($left.Count -eq 0) "install folder emptied (left: $(($left | ForEach-Object Name) -join ', '))"
  Remove-Item -LiteralPath $Dir -Recurse -Force -ErrorAction SilentlyContinue
}

function Assert-Installed([string] $Dir) {
  $expected = Get-RelativeFiles $payload
  $missing = @()
  if (Test-Path -LiteralPath $Dir) {
    $actual = Get-RelativeFiles $Dir
    $missing = @($expected | Where-Object { $actual -notcontains $_ })
  }
  else {
    $missing = $expected
  }
  Assert ($missing.Count -eq 0) "all $($expected.Count) payload files installed in $Dir (missing: $($missing -join ', '))"
  foreach ($file in $expected) {
    $installed = Join-Path $Dir $file
    if (Test-Path -LiteralPath $installed) {
      $same = (Get-FileHash -LiteralPath $installed).Hash -eq (Get-FileHash -LiteralPath (Join-Path $payload $file)).Hash
      Assert $same "$file matches the payload"
    }
  }
  Assert (Test-Path -LiteralPath (Join-Path $Dir 'Uninstall.exe')) "uninstaller written to $Dir"

  $exe = Join-Path $Dir 'heartwire.exe'
  $arp = Get-ItemProperty -Path $arpPath
  Assert ($arp.DisplayName -eq 'Heartwire') "DisplayName (got $($arp.DisplayName))"
  Assert ($arp.DisplayVersion -eq $Version) "DisplayVersion $Version (got $($arp.DisplayVersion))"
  Assert ($arp.Publisher -eq 'RealWhyKnot') "Publisher (got $($arp.Publisher))"
  Assert ($arp.InstallLocation -eq $Dir) "InstallLocation $Dir (got $($arp.InstallLocation))"
  Assert ($arp.DisplayIcon -eq $exe) "DisplayIcon (got $($arp.DisplayIcon))"
  Assert ($arp.UninstallString -eq ('"' + (Join-Path $Dir 'Uninstall.exe') + '"')) "UninstallString (got $($arp.UninstallString))"
  Assert ($arp.QuietUninstallString -eq ('"' + (Join-Path $Dir 'Uninstall.exe') + '" /S')) "QuietUninstallString (got $($arp.QuietUninstallString))"
  Assert ($arp.NoModify -eq 1 -and $arp.NoRepair -eq 1) "NoModify and NoRepair"
  Assert ($arp.EstimatedSize -gt 0) "EstimatedSize (got $($arp.EstimatedSize))"

  $target = ''
  $workingDir = ''
  if (Test-Path -LiteralPath $shortcut) {
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
    $target = $link.TargetPath
    $workingDir = $link.WorkingDirectory
  }
  Assert ($target -eq $exe) "start menu shortcut targets $exe (got '$target')"
  Assert ($workingDir -eq $Dir) "start menu shortcut starts in $Dir (got '$workingDir')"
}

$setupPath = (Resolve-Path -LiteralPath $Setup).Path
$payload = (Resolve-Path -LiteralPath $Payload).Path.TrimEnd('\', '/')
$arpPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Heartwire'
$runPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$shortcut = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Heartwire.lnk'
$dataDir = Join-Path $env:APPDATA 'heartwire'
$runtimeFiles = @('heartwire.vrmanifest', 'heartwire-icon.png')
Add-Type -Namespace InstallerTest -Name Native -MemberDefinition '[DllImport("kernel32.dll", CharSet = CharSet.Unicode)] public static extern uint GetLongPathName(string shortPath, System.Text.StringBuilder longPath, uint size);'
$longTemp = New-Object System.Text.StringBuilder 1024
if ([InstallerTest.Native]::GetLongPathName($env:TEMP, $longTemp, 1024) -eq 0) { throw "GetLongPathName failed for $env:TEMP" }
$root = Join-Path $longTemp.ToString() 'Heartwire Setup Smoke'
$first = Join-Path $root 'first'
$second = Join-Path $root 'second'

if (Test-Path -Path $arpPath) { throw "refusing to run: Heartwire is installed on this machine and the test would remove it" }
if (Test-Path -LiteralPath $shortcut) { throw "refusing to run: $shortcut exists and the test would remove it" }
if ($null -ne (Get-RunValue)) { throw "refusing to run: Heartwire starts with Windows on this machine and the test would change that" }
if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }

$keptSeed = Join-Path $dataDir 'setup-smoke.txt'
$stagingSeed = Join-Path $dataDir 'update\setup-smoke.bin'
$createdData = -not (Test-Path -LiteralPath $dataDir)
$realRoaming = $env:APPDATA
$realLocal = $env:LOCALAPPDATA
$fakeRoaming = Join-Path $root 'profile\Roaming'
$fakeLocal = Join-Path $root 'profile\Local'
$fakeSettings = Join-Path $fakeRoaming 'heartwire\config.json'

try {
  New-Item -ItemType Directory -Force -Path $fakeRoaming, $fakeLocal | Out-Null
  $env:APPDATA = $fakeRoaming
  $env:LOCALAPPDATA = $fakeLocal

  $code = Invoke-Exe $setupPath "/S /D=$first"
  Assert ("$code" -eq '0') "fresh install exit code 0 (got $code)"
  Assert-Installed $first
  Assert (-not (Test-Path -LiteralPath (Split-Path -Parent $fakeSettings))) "a fresh install creates no settings folder"

  Add-RuntimeFiles $first
  Set-RunValue (Join-Path $first 'heartwire.exe')
  $code = Invoke-Exe $setupPath "/S /D=$second"
  Assert ("$code" -eq '0') "install into a new folder exit code 0 (got $code)"
  Assert (-not (Test-Path -LiteralPath $first)) "moving the install removed the old folder"
  Assert-Installed $second
  $expectedRun = '"' + (Join-Path $second 'heartwire.exe') + '" --minimized'
  Assert ((Get-RunValue) -eq $expectedRun) "moving the install moved the start-up entry (got '$(Get-RunValue)')"

  $code = Invoke-Exe $setupPath "/S /D=$second"
  Assert ("$code" -eq '0') "reinstall over the same folder exit code 0 (got $code)"
  Assert-Installed $second
  Assert ((Get-RunValue) -eq $expectedRun) "reinstalling kept the start-up entry (got '$(Get-RunValue)')"

  $foreign = '"' + (Join-Path $root 'portable\heartwire.exe') + '" --minimized'
  New-ItemProperty -Path $runPath -Name 'Heartwire' -Value $foreign -PropertyType String -Force | Out-Null
  Add-RuntimeFiles $second
  Invoke-Uninstall $second
  Assert ((Get-RunValue) -eq $foreign) "uninstall kept a start-up entry that points at another copy (got '$(Get-RunValue)')"

  $code = Invoke-Exe $setupPath "/S /D=$second"
  Assert ("$code" -eq '0') "install after uninstall exit code 0 (got $code)"
  Assert-Installed $second

  $manifest = Join-Path $second 'heartwire.vrmanifest'
  $steamConfig = Join-Path $root 'profile\Steam\config'
  Write-Text $fakeSettings '{"steamvr_autostart": true, "steamvr_registered": false, "osc_client_port": 9123}'
  Write-Text (Join-Path $fakeLocal 'openvr\openvrpaths.vrpath') (ConvertTo-Json @{ config = @($steamConfig); runtime = @(); version = 1 })
  Write-Text (Join-Path $steamConfig 'appconfig.json') (ConvertTo-Json @{ manifest_paths = @($manifest) })
  $code = Invoke-Exe $setupPath "/S /D=$second"
  Assert ("$code" -eq '0') "reinstall with start with SteamVR on exit code 0 (got $code)"
  Assert (Test-Path -LiteralPath $manifest) "setup wrote the SteamVR manifest beside the exe"
  $settings = Get-Content -LiteralPath $fakeSettings -Raw | ConvertFrom-Json
  Assert ($settings.steamvr_registered -eq $true) "setup recorded the existing SteamVR registration (got $($settings.steamvr_registered))"
  Assert ($settings.steamvr_autostart -eq $true -and $settings.osc_client_port -eq 9123) "setup kept the other settings"
  $code = Invoke-Exe (Join-Path $second 'heartwire.exe') '--unregister-steamvr' 60
  Assert ("$code" -eq '3') "--unregister-steamvr exits 3 when SteamVR can't be loaded instead of opening the app (got $code)"

  Add-RuntimeFiles $second
  Set-RunValue (Join-Path $second 'heartwire.exe')
  foreach ($seed in @($keptSeed, $stagingSeed)) {
    New-Item -ItemType File -Force -Path $seed | Out-Null
  }
  Invoke-Uninstall $second
  Assert ($null -eq (Get-RunValue)) "uninstall removed its own start-up entry (got '$(Get-RunValue)')"
  Assert (Test-Path -LiteralPath $keptSeed) "silent uninstall kept $keptSeed"
  Assert (-not (Test-Path -LiteralPath (Split-Path -Parent $stagingSeed))) "uninstall removed the update staging folder"
}
finally {
  $env:APPDATA = $realRoaming
  $env:LOCALAPPDATA = $realLocal
  if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
  if (Test-Path -LiteralPath $keptSeed) { Remove-Item -LiteralPath $keptSeed -Force }
  if (Test-Path -LiteralPath $stagingSeed) { Remove-Item -LiteralPath (Split-Path -Parent $stagingSeed) -Recurse -Force }
  if ($createdData -and (Test-Path -LiteralPath $dataDir) -and @(Get-ChildItem -LiteralPath $dataDir -Force).Count -eq 0) { Remove-Item -LiteralPath $dataDir -Force }
  Remove-ItemProperty -Path $runPath -Name 'Heartwire' -ErrorAction SilentlyContinue
  if (Test-Path -Path $arpPath) { Remove-Item -Path $arpPath -Recurse -Force }
  if (Test-Path -LiteralPath $shortcut) { Remove-Item -LiteralPath $shortcut -Force }
}

if ($script:failures.Count -gt 0) {
  throw "installer test failed: $($script:failures -join '; ')"
}
Write-Host "installer test passed"
