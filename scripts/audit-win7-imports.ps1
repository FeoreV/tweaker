param(
    [Parameter(Mandatory = $true)][string]$Binary,
    [Parameter(Mandatory = $true)][string]$Dumpbin
)
$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $Binary -PathType Leaf)) { throw "Binary not found: $Binary" }
if (-not (Test-Path -LiteralPath $Dumpbin -PathType Leaf)) { throw "dumpbin not found: $Dumpbin" }
$imports = & $Dumpbin /imports $Binary
if ($LASTEXITCODE -ne 0) { throw "dumpbin /imports failed: $LASTEXITCODE" }
if (-not ($imports | Select-String -Pattern '^\s+\S+\.dll\s*$')) { throw 'No DLL imports found; cannot verify output.' }
$forbidden = '(?i)\b(WaitOnAddress|WakeByAddressSingle|WakeByAddressAll|CreateFile2|SetThreadDescription)\b|^\s*(srclient|vcruntime\d*|msvcp\d*|ucrtbase|api-ms-win-crt-[^\s]*)\.dll\s*$'
$violations = @($imports | Select-String -Pattern $forbidden)
if ($violations.Count -gt 0) { $violations | ForEach-Object { Write-Error $_.Line -ErrorAction Continue }; throw 'Forbidden import or dynamic CRT detected.' }
$headers = & $Dumpbin /headers $Binary
if ($LASTEXITCODE -ne 0) { throw "dumpbin /headers failed: $LASTEXITCODE" }
if (-not ($headers | Select-String -Pattern '^\s*6\.01 subsystem version\s*$')) { throw 'Expected subsystem version 6.01.' }
Write-Output "Selected import and subsystem checks passed: $Binary"
