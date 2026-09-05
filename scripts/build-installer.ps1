param(
    [string]$Payload,
    [string]$OutputDirectory,
    [switch]$Preview
)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$config = Get-Content (Join-Path $repoRoot 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
$version = $config.version
if (!$OutputDirectory) { $OutputDirectory = Join-Path $repoRoot 'artifacts/installer' }
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$framework = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319'
$compiler = Join-Path $framework 'csc.exe'
if (!(Test-Path $compiler)) { throw 'Building the installer requires Windows and .NET Framework 4.x.' }
if (!$Payload -and !$Preview) {
    $Payload = Join-Path $repoRoot "src-tauri/target/release/bundle/nsis/$($config.productName)_${version}_x64-setup.exe"
}
if (!$Preview) {
    $Payload = (Resolve-Path -LiteralPath $Payload).Path
    $payloadVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($Payload).ProductVersion
    if ($payloadVersion -ne $version -and $payloadVersion -ne "$version.0") {
        throw "Payload version '$payloadVersion' does not match app version '$version'. Build the NSIS bundle first."
    }
    $hash = (Get-FileHash -LiteralPath $Payload -Algorithm SHA256).Hash
} else { $hash = 'PREVIEW_ONLY' }
$generated = Join-Path $OutputDirectory 'BuildInfo.cs'
$previewLiteral = if ($Preview) { 'true' } else { 'false' }
@"
using System.Reflection;
[assembly: AssemblyTitle("ZAPRET Installer")]
[assembly: AssemblyProduct("ZAPRET")]
[assembly: AssemblyVersion("$version.0")]
[assembly: AssemblyFileVersion("$version.0")]
namespace ZapretSetup { internal static class BuildInfo {
    internal const string Version = "$version";
    internal const string PayloadHash = "$hash";
    internal const bool PreviewOnly = $previewLiteral;
} }
"@ | Set-Content -LiteralPath $generated -Encoding UTF8
# WPF pack resources keep the application's actual Inter font inside the single executable.
$resourceFile = Join-Path $OutputDirectory 'ZapretSetup.g.resources'
$writer = New-Object System.Resources.ResourceWriter($resourceFile)
try {
    foreach ($weight in @('400', '600', '700')) {
        $fontPath = Join-Path $repoRoot "src/assets/fonts/inter-$weight.ttf"
        $stream = [IO.File]::OpenRead($fontPath)
        $writer.AddResource("fonts/inter-$weight.ttf", $stream, $true)
    }
    $writer.Generate()
} finally { $writer.Dispose() }
$name = if ($Preview) { "ZAPRET_${version}_x64-setup-preview.exe" } else { "ZAPRET_${version}_x64-branded-setup.exe" }
$assemblyName = [IO.Path]::GetFileNameWithoutExtension($name)
$output = Join-Path $OutputDirectory $name
$compilerArgs = @('/nologo', '/target:winexe', '/platform:x64', '/optimize+', '/utf8output',
    "/out:$output", "/win32icon:$(Join-Path $repoRoot 'src-tauri/icons/icon.ico')",
    "/win32manifest:$(Join-Path $repoRoot 'installer/app.manifest')",
    "/resource:$(Join-Path $repoRoot 'installer/Installer.xaml'),Installer.xaml",
    "/resource:$(Join-Path $repoRoot 'src-tauri/icons/128x128.png'),icon.png",
    "/resource:$resourceFile,$assemblyName.g.resources",
    '/reference:System.dll', '/reference:System.Core.dll', '/reference:System.Xaml.dll', '/reference:System.Windows.Forms.dll',
    "/reference:$framework/WPF/WindowsBase.dll", "/reference:$framework/WPF/PresentationCore.dll", "/reference:$framework/WPF/PresentationFramework.dll")
if (!$Preview) { $compilerArgs += "/resource:$Payload,payload.exe" }
$compilerArgs += @($generated, (Join-Path $repoRoot 'installer/Program.cs'), (Join-Path $repoRoot 'installer/InstallerEngine.cs'), (Join-Path $repoRoot 'installer/UpdateOptions.cs'))
& $compiler @compilerArgs
if ($LASTEXITCODE -ne 0) { throw "Installer compilation failed ($LASTEXITCODE)." }
Write-Output $output
