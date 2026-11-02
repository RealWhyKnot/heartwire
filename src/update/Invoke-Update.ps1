#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateRange(1, 2147483647)][int] $ParentProcessId,
    [Parameter(Mandatory)][ValidateSet('Archive', 'Setup')][string] $Package,
    [Parameter(Mandatory)][string] $File,
    [Parameter(Mandatory)][string] $Staging,
    [Parameter(Mandatory)][string] $Install,
    [Parameter(Mandatory)][string] $Executable,
    [Parameter(Mandatory)][string] $Log
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Copy-UpdateFile {
    [CmdletBinding()]
    [OutputType([void])]
    param([string] $Source, [string] $Destination)

    $attempt = 0
    while ($true) {
        try {
            Copy-Item -Path (Join-Path -Path $Source -ChildPath '*') -Destination $Destination -Recurse -Force
            return
        } catch [System.IO.IOException], [System.UnauthorizedAccessException],
        [System.Management.Automation.RuntimeException], [System.ArgumentException] {
            $attempt++
            if ($attempt -ge 10) {
                throw
            }
            Start-Sleep -Seconds 1
        }
    }
}

$applied = $false
try {
    Wait-Process -Id $ParentProcessId -Timeout 120 -ErrorAction SilentlyContinue
    if ($Package -eq 'Setup') {
        $start = @{
            FilePath     = $File
            ArgumentList = "/S /D=$Install"
            Wait         = $true
            PassThru     = $true
        }
        $setup = Start-Process @start
        if ($setup.ExitCode -ne 0) {
            throw "setup exited with code $($setup.ExitCode)"
        }
    } else {
        $extracted = Join-Path -Path $Staging -ChildPath 'extracted'
        Expand-Archive -LiteralPath $File -DestinationPath $extracted -Force
        Copy-UpdateFile -Source $extracted -Destination $Install
    }
    $applied = $true
} catch [System.IO.IOException], [System.UnauthorizedAccessException], [System.ArgumentException],
[System.InvalidOperationException], [System.ComponentModel.Win32Exception],
[System.Management.Automation.RuntimeException] {
    [IO.File]::AppendAllText($Log, 'update apply failed' + [Environment]::NewLine)
    [IO.File]::AppendAllText($Log, ($_ | Out-String) + [Environment]::NewLine)
}
try {
    Start-Process -FilePath $Executable -WorkingDirectory $Install
} catch [System.IO.IOException], [System.UnauthorizedAccessException], [System.ArgumentException],
[System.InvalidOperationException], [System.ComponentModel.Win32Exception],
[System.Management.Automation.RuntimeException] {
    [IO.File]::AppendAllText($Log, ($_ | Out-String) + [Environment]::NewLine)
}
Remove-Item -LiteralPath $Staging -Recurse -Force -ErrorAction SilentlyContinue
if (-not $applied) {
    exit 1
}
exit 0
