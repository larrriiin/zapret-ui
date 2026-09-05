$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$testRoot = Join-Path $repoRoot 'artifacts/installer/tests'
New-Item -ItemType Directory -Force -Path $testRoot | Out-Null
$compiler = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
$payload = Join-Path $testRoot 'fake-payload.exe'
& $compiler /nologo /target:exe "/out:$payload" (Join-Path $repoRoot 'installer/tests/FakePayload.cs')
if ($LASTEXITCODE -ne 0) { throw 'Fake payload compilation failed.' }
$hash = (Get-FileHash $payload -Algorithm SHA256).Hash
$info = Join-Path $testRoot 'BuildInfo.cs'
"namespace ZapretSetup { internal static class BuildInfo { internal const string PayloadHash = `"$hash`"; } }" | Set-Content $info
$test = Join-Path $testRoot 'EngineTests.exe'
& $compiler /nologo /target:exe "/out:$test" "/resource:$payload,payload.exe" $info (Join-Path $repoRoot 'installer/InstallerEngine.cs') (Join-Path $repoRoot 'installer/UpdateOptions.cs') (Join-Path $repoRoot 'installer/tests/EngineTests.cs')
if ($LASTEXITCODE -ne 0) { throw 'Test compilation failed.' }
& $test $testRoot
if ($LASTEXITCODE -ne 0) { throw 'Installer engine checks failed.' }
"namespace ZapretSetup { internal static class BuildInfo { internal const string PayloadHash = `"INVALID_HASH`"; } }" | Set-Content $info
$integrityTest = Join-Path $testRoot 'IntegrityTests.exe'
& $compiler /nologo /target:exe "/out:$integrityTest" "/resource:$payload,payload.exe" $info (Join-Path $repoRoot 'installer/InstallerEngine.cs') (Join-Path $repoRoot 'installer/UpdateOptions.cs') (Join-Path $repoRoot 'installer/tests/IntegrityTests.cs')
if ($LASTEXITCODE -ne 0) { throw 'Integrity test compilation failed.' }
& $integrityTest $testRoot
if ($LASTEXITCODE -ne 0) { throw 'Installer integrity check failed.' }
