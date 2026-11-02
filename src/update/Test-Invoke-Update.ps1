#!/usr/bin/env pwsh
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Assert-UpdateSyntax {
    [CmdletBinding()]
    [OutputType([void])]
    param([bool] $Condition, [string] $Message)

    if (-not $Condition) {
        [Console]::Error.WriteLine($Message)
        $script:failures++
    }
}

$script:failures = 0
$tokens = $null
$parseErrors = $null
$path = Join-Path -Path $PSScriptRoot -ChildPath 'Invoke-Update.ps1'
$ast = [System.Management.Automation.Language.Parser]::ParseFile($path, [ref] $tokens, [ref] $parseErrors)
Assert-UpdateSyntax -Condition (@($parseErrors).Count -eq 0) -Message ($parseErrors | Out-String)
$names = @($ast.ParamBlock.Parameters.Name.VariablePath.UserPath)
foreach ($name in @('ParentProcessId', 'Package', 'File', 'Staging', 'Install', 'Executable', 'Log')) {
    Assert-UpdateSyntax -Condition ($names -contains $name) -Message "Missing updater parameter: $name"
}
$invocation = @(
    "& './Invoke-Update.ps1'"
    '-ParentProcessId 42 -Package Archive'
    '-File "C:/it''s here/update.zip"'
    '-Staging "C:/it''s here/staging"'
    '-Install "C:/it''s here/install dir"'
    '-Executable "C:/it''s here/install dir/heartwire.exe"'
    '-Log "C:/it''s here/update.log"'
) -join ' '
$null = [System.Management.Automation.Language.Parser]::ParseInput($invocation, [ref] $tokens, [ref] $parseErrors)
Assert-UpdateSyntax -Condition (@($parseErrors).Count -eq 0) -Message ($parseErrors | Out-String)
if ($script:failures -ne 0) {
    exit 1
}
exit 0
