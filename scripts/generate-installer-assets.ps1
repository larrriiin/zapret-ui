$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$repoRoot = Split-Path -Parent $PSScriptRoot
$assetDirectory = Join-Path $repoRoot 'installer/assets'
New-Item -ItemType Directory -Force $assetDirectory | Out-Null
$fonts = New-Object System.Drawing.Text.PrivateFontCollection
$fonts.AddFontFile((Join-Path $repoRoot 'src/assets/fonts/inter-700.ttf'))
$font = New-Object System.Drawing.Font($fonts.Families[0], 19, [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
$accent = New-Object System.Drawing.SolidBrush([System.Drawing.ColorTranslator]::FromHtml('#BA9EFF'))
$icon = [System.Drawing.Image]::FromFile((Join-Path $repoRoot 'src-tauri/icons/128x128.png'))
try {
    foreach ($layout in @(@{Name='sidebar';Width=164;Height=314}, @{Name='header';Width=150;Height=57})) {
        $bitmap = New-Object System.Drawing.Bitmap($layout.Width, $layout.Height, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.Clear([System.Drawing.ColorTranslator]::FromHtml('#070D1F'))
            $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $graphics.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
            if ($layout.Name -eq 'sidebar') {
                $graphics.DrawImage($icon, 28, 32, 80, 80)
                $graphics.DrawString('ZAPRET_', $font, $accent, 24, 126)
            } else {
                $graphics.DrawImage($icon, 11, 12, 32, 32)
                $graphics.DrawString('ZAPRET', $font, $accent, 49, 16)
            }
            $bitmap.Save((Join-Path $assetDirectory "$($layout.Name).bmp"), [System.Drawing.Imaging.ImageFormat]::Bmp)
        } finally { $graphics.Dispose(); $bitmap.Dispose() }
    }
} finally { $icon.Dispose(); $font.Dispose(); $fonts.Dispose(); $accent.Dispose() }
