#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string] $Setup,
    [Parameter(Mandatory = $true)][string] $Payload,
    [Parameter(Mandatory = $true)][string] $Version
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Test-InstallerCondition {
    [CmdletBinding()]
    [OutputType([void])]
    param([bool] $Condition, [string] $Message)
    if ($Condition) {
        Write-Host "ok   $Message"
    } else {
        $script:failures.Add($Message)
        Write-Host "FAIL $Message"
    }
}

function Invoke-Executable {
    [CmdletBinding()]
    [OutputType([int], [string])]
    param([string] $Path, [string] $Arguments, [ValidateRange(1, 2147483)][int] $TimeoutSeconds = 180)
    $process = Start-Process -FilePath $Path -ArgumentList $Arguments -PassThru -WindowStyle Hidden
    $null = $process.Handle
    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
        Stop-Process -Id $process.Id -Force
        return 'timeout'
    }
    return $process.ExitCode
}

function Get-RelativeFile {
    [CmdletBinding()]
    [OutputType([object[]])]
    param([string] $Root)
    $full = (Resolve-Path -LiteralPath $Root).Path.TrimEnd('\', '/')
    return @(Get-ChildItem -LiteralPath $full -File -Recurse -Force |
            ForEach-Object { $_.FullName.Substring($full.Length + 1) } | Sort-Object)
}

function Get-RunValue {
    [CmdletBinding()]
    [OutputType([string])]
    param()
    $values = Get-ItemProperty -LiteralPath $runPath -ErrorAction SilentlyContinue
    if ($values -and $values.PSObject.Properties['Heartwire']) {
        return $values.PSObject.Properties['Heartwire'].Value
    }
    return $null
}

function Set-RunValue {
    [CmdletBinding()]
    [OutputType([void])]
    param([string] $Exe)
    if (-not (Test-Path -LiteralPath $runPath)) { $null = New-Item -Path $runPath -Force }
    $property = @{
        LiteralPath  = $runPath
        Name         = 'Heartwire'
        Value        = '"' + $Exe + '" --minimized'
        PropertyType = 'String'
        Force        = $true
    }
    $null = New-ItemProperty @property
}

function Add-RuntimeFile {
    [CmdletBinding()]
    [OutputType([void])]
    param([string] $Dir)
    foreach ($name in $runtimeFiles) {
        $null = New-Item -ItemType File -Force -Path (Join-Path -Path $Dir -ChildPath $name)
    }
}

function Write-Text {
    [CmdletBinding()]
    [OutputType([void])]
    param([string] $Path, [string] $Text)
    $null = New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Path)
    [System.IO.File]::WriteAllText($Path, $Text, [Text.UTF8Encoding]::new($false))
}

function Invoke-Uninstall {
    [CmdletBinding()]
    [OutputType([void])]
    param([string] $Dir)
    $code = Invoke-Executable -Path (Join-Path -Path $Dir -ChildPath 'Uninstall.exe') -Arguments "/S _?=$Dir"
    Test-InstallerCondition -Condition ("$code" -eq '0') -Message "uninstaller exit code 0 (got $code)"
    Test-InstallerCondition -Condition (-not (Test-Path -LiteralPath $arpPath)) -Message 'uninstall entry removed'
    Test-InstallerCondition -Condition (-not (Test-Path -LiteralPath $shortcut)) -Message 'start menu shortcut removed'
    $left = @()
    if (Test-Path -LiteralPath $Dir) {
        $left = @(Get-ChildItem -LiteralPath $Dir -Recurse -Force | Where-Object { $_.Name -ne 'Uninstall.exe' })
    }
    $check = @{
        Condition = ($left.Count -eq 0)
        Message   = "install folder emptied (left: $(@(foreach ($item in $left) { $item.Name }) -join ', '))"
    }
    Test-InstallerCondition @check
    Remove-Item -LiteralPath $Dir -Recurse -Force -ErrorAction SilentlyContinue
}

function Test-Installed {
    [CmdletBinding()]
    [OutputType([void])]
    param([string] $Dir)
    $expected = @(Get-RelativeFile -Root $payload)
    $missing = @()
    if (Test-Path -LiteralPath $Dir) {
        $actual = @(Get-RelativeFile -Root $Dir)
        $missing = @($expected | Where-Object { $actual -notcontains $_ })
    } else {
        $missing = $expected
    }
    $check = @{
        Condition = ($missing.Count -eq 0)
        Message   = "all $($expected.Count) payload files installed in $Dir (missing: $($missing -join ', '))"
    }
    Test-InstallerCondition @check
    foreach ($file in $expected) {
        $installed = Join-Path -Path $Dir -ChildPath $file
        if (Test-Path -LiteralPath $installed) {
            $sourceFile = Join-Path -Path $payload -ChildPath $file
            $same = (Get-FileHash -LiteralPath $installed).Hash -eq (Get-FileHash -LiteralPath $sourceFile).Hash
            Test-InstallerCondition -Condition $same -Message "$file matches the payload"
        }
    }
    $check = @{
        Condition = (Test-Path -LiteralPath (Join-Path -Path $Dir -ChildPath 'Uninstall.exe'))
        Message   = "uninstaller written to $Dir"
    }
    Test-InstallerCondition @check

    $exe = Join-Path -Path $Dir -ChildPath 'heartwire.exe'
    $arp = Get-ItemProperty -LiteralPath $arpPath
    $check = @{
        Condition = ($arp.DisplayName -eq 'Heartwire')
        Message   = "DisplayName (got $($arp.DisplayName))"
    }
    Test-InstallerCondition @check
    $check = @{
        Condition = ($arp.DisplayVersion -eq $script:expectedVersion)
        Message   = "DisplayVersion $Version (got $($arp.DisplayVersion))"
    }
    Test-InstallerCondition @check
    Test-InstallerCondition -Condition ($arp.Publisher -eq 'RealWhyKnot') -Message "Publisher (got $($arp.Publisher))"
    $check = @{
        Condition = ($arp.InstallLocation -eq $Dir)
        Message   = "InstallLocation $Dir (got $($arp.InstallLocation))"
    }
    Test-InstallerCondition @check
    Test-InstallerCondition -Condition ($arp.DisplayIcon -eq $exe) -Message "DisplayIcon (got $($arp.DisplayIcon))"
    $check = @{
        Condition = ($arp.UninstallString -eq ('"' + (Join-Path -Path $Dir -ChildPath 'Uninstall.exe') + '"'))
        Message   = "UninstallString (got $($arp.UninstallString))"
    }
    Test-InstallerCondition @check
    $check = @{
        Condition = ($arp.QuietUninstallString -eq ('"' + (Join-Path -Path $Dir -ChildPath 'Uninstall.exe') + '" /S'))
        Message   = "QuietUninstallString (got $($arp.QuietUninstallString))"
    }
    Test-InstallerCondition @check
    Test-InstallerCondition -Condition ($arp.NoModify -eq 1 -and $arp.NoRepair -eq 1) -Message 'NoModify and NoRepair'
    Test-InstallerCondition -Condition ($arp.EstimatedSize -gt 0) -Message "EstimatedSize (got $($arp.EstimatedSize))"

    $target = ''
    $workingDir = ''
    if (Test-Path -LiteralPath $shortcut) {
        $link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
        $target = $link.TargetPath
        $workingDir = $link.WorkingDirectory
    }
    Test-InstallerCondition -Condition ($target -eq $exe) -Message "start menu shortcut targets $exe (got '$target')"
    $check = @{
        Condition = ($workingDir -eq $Dir)
        Message   = "start menu shortcut starts in $Dir (got '$workingDir')"
    }
    Test-InstallerCondition @check
}

$script:failures = [Collections.Generic.List[string]]::new()
$script:expectedVersion = $Version
$setupPath = (Resolve-Path -LiteralPath $Setup).Path
$payload = (Resolve-Path -LiteralPath $Payload).Path.TrimEnd('\', '/')
$arpPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Heartwire'
$runPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$shortcut = Join-Path -Path $env:APPDATA -ChildPath 'Microsoft\Windows\Start Menu\Programs\Heartwire.lnk'
$dataDir = Join-Path -Path $env:APPDATA -ChildPath 'heartwire'
$runtimeFiles = @('heartwire.vrmanifest', 'heartwire-icon.png')
$native = @{
    Namespace        = 'InstallerTest'
    Name             = 'Native'
    MemberDefinition = @'
[DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
public static extern uint GetLongPathName(string shortPath, System.Text.StringBuilder longPath, uint size);
'@
}
Add-Type @native
$longTemp = [Text.StringBuilder]::new(1024)
if ([InstallerTest.Native]::GetLongPathName($env:TEMP, $longTemp, 1024) -eq 0) {
    throw "GetLongPathName failed for $env:TEMP"
}
$root = Join-Path -Path $longTemp.ToString() -ChildPath ('Heartwire Setup Smoke-' + [guid]::NewGuid().ToString('N'))
$first = Join-Path -Path $root -ChildPath 'first'
$second = Join-Path -Path $root -ChildPath 'second'

if (Test-Path -LiteralPath $arpPath) {
    throw 'refusing to run: Heartwire is installed on this machine and the test would remove it'
}
if (Test-Path -LiteralPath $shortcut) { throw "refusing to run: $shortcut exists and the test would remove it" }
if ($null -ne (Get-RunValue)) {
    throw 'refusing to run: Heartwire starts with Windows on this machine and the test would change that'
}
if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }

$keptSeed = Join-Path -Path $dataDir -ChildPath 'setup-smoke.txt'
$stagingSeed = Join-Path -Path $dataDir -ChildPath 'update\setup-smoke.bin'
$createdData = -not (Test-Path -LiteralPath $dataDir)
$realRoaming = $env:APPDATA
$realLocal = $env:LOCALAPPDATA
$fakeRoaming = Join-Path -Path $root -ChildPath 'profile\Roaming'
$fakeLocal = Join-Path -Path $root -ChildPath 'profile\Local'
$fakeSettings = Join-Path -Path $fakeRoaming -ChildPath 'heartwire\config.json'

try {
    $null = New-Item -ItemType Directory -Force -Path $fakeRoaming, $fakeLocal
    $env:APPDATA = $fakeRoaming
    $env:LOCALAPPDATA = $fakeLocal

    $code = Invoke-Executable -Path $setupPath -Arguments "/S /D=$first"
    Test-InstallerCondition -Condition ("$code" -eq '0') -Message "fresh install exit code 0 (got $code)"
    Test-Installed -Dir $first
    $check = @{
        Condition = (-not (Test-Path -LiteralPath (Split-Path -Parent $fakeSettings)))
        Message   = 'a fresh install creates no settings folder'
    }
    Test-InstallerCondition @check

    Add-RuntimeFile -Dir $first
    Set-RunValue -Exe (Join-Path -Path $first -ChildPath 'heartwire.exe')
    $code = Invoke-Executable -Path $setupPath -Arguments "/S /D=$second"
    Test-InstallerCondition -Condition ("$code" -eq '0') -Message "install into a new folder exit code 0 (got $code)"
    $check = @{
        Condition = (-not (Test-Path -LiteralPath $first))
        Message   = 'moving the install removed the old folder'
    }
    Test-InstallerCondition @check
    Test-Installed -Dir $second
    $expectedRun = '"' + (Join-Path -Path $second -ChildPath 'heartwire.exe') + '" --minimized'
    $check = @{
        Condition = ((Get-RunValue) -eq $expectedRun)
        Message   = "moving the install moved the start-up entry (got '$(Get-RunValue)')"
    }
    Test-InstallerCondition @check

    $code = Invoke-Executable -Path $setupPath -Arguments "/S /D=$second"
    $check = @{
        Condition = ("$code" -eq '0')
        Message   = "reinstall over the same folder exit code 0 (got $code)"
    }
    Test-InstallerCondition @check
    Test-Installed -Dir $second
    $check = @{
        Condition = ((Get-RunValue) -eq $expectedRun)
        Message   = "reinstalling kept the start-up entry (got '$(Get-RunValue)')"
    }
    Test-InstallerCondition @check

    $foreign = '"' + (Join-Path -Path $root -ChildPath 'portable\heartwire.exe') + '" --minimized'
    $null = New-ItemProperty -LiteralPath $runPath -Name 'Heartwire' -Value $foreign -PropertyType String -Force
    Add-RuntimeFile -Dir $second
    Invoke-Uninstall -Dir $second
    $check = @{
        Condition = ((Get-RunValue) -eq $foreign)
        Message   = "uninstall kept a start-up entry that points at another copy (got '$(Get-RunValue)')"
    }
    Test-InstallerCondition @check

    $code = Invoke-Executable -Path $setupPath -Arguments "/S /D=$second"
    Test-InstallerCondition -Condition ("$code" -eq '0') -Message "install after uninstall exit code 0 (got $code)"
    Test-Installed -Dir $second

    $manifest = Join-Path -Path $second -ChildPath 'heartwire.vrmanifest'
    $steamConfig = Join-Path -Path $root -ChildPath 'profile\Steam\config'
    $settingsSeed = '{"steamvr_autostart": true, "steamvr_registered": false, "osc_client_port": 9123}'
    Write-Text -Path $fakeSettings -Text $settingsSeed
    $runtimePath = Join-Path -Path $fakeLocal -ChildPath 'openvr\openvrpaths.vrpath'
    $runtimeConfig = @{ config = @($steamConfig); runtime = @(); version = 1 }
    Write-Text -Path $runtimePath -Text (ConvertTo-Json -Depth 10 -InputObject $runtimeConfig)
    $applicationPath = Join-Path -Path $steamConfig -ChildPath 'appconfig.json'
    Write-Text -Path $applicationPath -Text (ConvertTo-Json -Depth 10 -InputObject @{ manifest_paths = @($manifest) })
    $code = Invoke-Executable -Path $setupPath -Arguments "/S /D=$second"
    $check = @{
        Condition = ("$code" -eq '0')
        Message   = "reinstall with start with SteamVR on exit code 0 (got $code)"
    }
    Test-InstallerCondition @check
    $check = @{
        Condition = (Test-Path -LiteralPath $manifest)
        Message   = 'setup wrote the SteamVR manifest beside the exe'
    }
    Test-InstallerCondition @check
    $settings = [IO.File]::ReadAllText($fakeSettings) | ConvertFrom-Json
    $check = @{
        Condition = ($settings.steamvr_registered -eq $true)
        Message   = "setup recorded the existing SteamVR registration (got $($settings.steamvr_registered))"
    }
    Test-InstallerCondition @check
    $check = @{
        Condition = ($settings.steamvr_autostart -eq $true -and $settings.osc_client_port -eq 9123)
        Message   = 'setup kept the other settings'
    }
    Test-InstallerCondition @check
    $unregister = @{
        Path           = Join-Path -Path $second -ChildPath 'heartwire.exe'
        Arguments      = '--unregister-steamvr'
        TimeoutSeconds = 60
    }
    $code = Invoke-Executable @unregister
    $check = @{
        Condition = ("$code" -eq '3')
        Message   = "--unregister-steamvr exits 3 when SteamVR can't be loaded instead of opening the app (got $code)"
    }
    Test-InstallerCondition @check

    Add-RuntimeFile -Dir $second
    Set-RunValue -Exe (Join-Path -Path $second -ChildPath 'heartwire.exe')
    foreach ($seed in @($keptSeed, $stagingSeed)) {
        $null = New-Item -ItemType File -Force -Path $seed
    }
    Invoke-Uninstall -Dir $second
    $check = @{
        Condition = ($null -eq (Get-RunValue))
        Message   = "uninstall removed its own start-up entry (got '$(Get-RunValue)')"
    }
    Test-InstallerCondition @check
    Test-InstallerCondition -Condition (Test-Path -LiteralPath $keptSeed) -Message "silent uninstall kept $keptSeed"
    $check = @{
        Condition = (-not (Test-Path -LiteralPath (Split-Path -Parent $stagingSeed)))
        Message   = 'uninstall removed the update staging folder'
    }
    Test-InstallerCondition @check
} finally {
    $env:APPDATA = $realRoaming
    $env:LOCALAPPDATA = $realLocal
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
    if (Test-Path -LiteralPath $keptSeed) { Remove-Item -LiteralPath $keptSeed -Force }
    if (Test-Path -LiteralPath $stagingSeed) {
        Remove-Item -LiteralPath (Split-Path -Parent $stagingSeed) -Recurse -Force
    }
    if ($createdData -and (Test-Path -LiteralPath $dataDir)) {
        if (@(Get-ChildItem -LiteralPath $dataDir -Force).Count -eq 0) {
            Remove-Item -LiteralPath $dataDir -Force
        }
    }
    Remove-ItemProperty -LiteralPath $runPath -Name 'Heartwire' -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $arpPath) { Remove-Item -LiteralPath $arpPath -Recurse -Force }
    if (Test-Path -LiteralPath $shortcut) { Remove-Item -LiteralPath $shortcut -Force }
}

if ($script:failures.Count -gt 0) {
    throw "installer test failed: $($script:failures -join '; ')"
}
Write-Host 'installer test passed'

exit 0
