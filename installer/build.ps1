#!/usr/bin/env pwsh
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string] $Payload,
  [Parameter(Mandatory = $true)][string] $Version,
  [Parameter(Mandatory = $true)][string] $OutFile,
  [string] $MakeNsis = (Join-Path ${env:ProgramFiles(x86)} 'NSIS\makensis.exe')
)

$ErrorActionPreference = 'Stop'
$payloadDir = (Resolve-Path -LiteralPath $Payload).Path.TrimEnd('\', '/')
if (-not (Test-Path -LiteralPath $MakeNsis)) { throw "makensis not found at $MakeNsis (install NSIS 3 from https://nsis.sourceforge.io)" }

$numbers = @($Version -split '[^0-9]+' | Where-Object { $_ -ne '' } | Select-Object -First 4)
while ($numbers.Count -lt 4) { $numbers += '0' }
$out = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutFile)
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $out) | Out-Null

& $MakeNsis /V2 /WX "/DVERSION=$Version" "/DVIVERSION=$($numbers -join '.')" "/DPAYLOAD=$payloadDir" "/DOUTFILE=$out" (Join-Path $PSScriptRoot 'installer.nsi')
if ($LASTEXITCODE -ne 0) { throw "makensis failed ($LASTEXITCODE)" }
Write-Host "Built $out"
