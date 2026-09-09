param([switch]$Elevated)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$testDirectory = Join-Path $projectRoot 'src-tauri\target\debug\deps'
$testBinary = Get-ChildItem -LiteralPath $testDirectory -Filter 'zapret_ui_lib-*.exe' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $testBinary) { throw 'Build the Rust library tests first.' }
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    if ($Elevated) { throw 'Administrator privileges were not granted.' }
    Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') -Verb RunAs -WindowStyle Hidden -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"' + $PSCommandPath + '"'), '-Elevated')
    exit
}
$resultPath = Join-Path $projectRoot 'artifacts\warp-tcp-smoke.log'
Set-Location -LiteralPath $projectRoot
$errorPath = Join-Path $projectRoot 'artifacts\warp-tcp-smoke-error.log'
$testProcess = Start-Process -FilePath $testBinary.FullName -ArgumentList @('--ignored', '--exact', 'providers::warp::applications::tcp::tests::transparent_tcp_smoke', '--nocapture') -WindowStyle Hidden -Wait -PassThru -RedirectStandardOutput $resultPath -RedirectStandardError $errorPath
$testExitCode = $testProcess.ExitCode
Add-Content -LiteralPath $resultPath -Value "Exit code: $testExitCode"
exit $testExitCode
