param([switch]$Unsigned)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    $buildArgs = @('node_modules/@tauri-apps/cli/tauri.js', 'build', '--bundles', 'nsis')
    if ($Unsigned) {
        New-Item -ItemType Directory -Force artifacts/installer | Out-Null
        '{"bundle":{"createUpdaterArtifacts":false}}' | Set-Content artifacts/installer/local-build.json -Encoding UTF8
        $buildArgs += @('--config', 'artifacts/installer/local-build.json')
    }
    & node @buildArgs
    if ($LASTEXITCODE -ne 0) { throw 'Native NSIS build failed.' }
} finally { Pop-Location }
