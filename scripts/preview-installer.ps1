param([string]$MakeNsis = (Join-Path $env:LOCALAPPDATA 'tauri/NSIS/makensis.exe'))
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
if (!(Test-Path -LiteralPath $MakeNsis)) { throw 'NSIS compiler not found. Pass -MakeNsis <path to makensis.exe> after building a Tauri NSIS bundle.' }
New-Item -ItemType Directory -Force (Join-Path $repoRoot 'artifacts/installer') | Out-Null
& $MakeNsis /V2 "/DPROJECT_ROOT=$repoRoot" (Join-Path $repoRoot 'installer/preview.nsi')
if ($LASTEXITCODE -ne 0) { throw 'NSIS preview compilation failed.' }
Write-Output (Join-Path $repoRoot 'artifacts/installer/nsis-preview.exe')
