#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string] $Payload,
    [Parameter(Mandatory = $true)][string] $Version,
    [Parameter(Mandatory = $true)][string] $OutFile,
    [string] $MakeNsis = (Join-Path -Path ${env:ProgramFiles(x86)} -ChildPath 'NSIS\makensis.exe')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$payloadDir = (Resolve-Path -LiteralPath $Payload).Path.TrimEnd('\', '/')
if (-not (Test-Path -LiteralPath $MakeNsis)) {
    throw "makensis not found at $MakeNsis (install NSIS 3 from https://nsis.sourceforge.io)"
}

$numbers = @($Version -split '[^0-9]+' | Where-Object { $_ -ne '' } | Select-Object -First 4)
$numbers = @(
    $numbers
    for ($index = $numbers.Count; $index -lt 4; $index++) { '0' }
)
$out = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutFile)
$null = [IO.Directory]::CreateDirectory((Split-Path -Parent $out))

$arguments = @(
    '/V2'
    '/WX'
    "/DVERSION=$Version"
    "/DVIVERSION=$($numbers -join '.')"
    "/DPAYLOAD=$payloadDir"
    "/DOUTFILE=$out"
    (Join-Path -Path $PSScriptRoot -ChildPath 'installer.nsi')
)
& $MakeNsis @arguments
if ($LASTEXITCODE -ne 0) { throw "makensis failed ($LASTEXITCODE)" }
Write-Host "Built $out"
exit 0
