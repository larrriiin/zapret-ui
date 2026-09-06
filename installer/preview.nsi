; Visual-only mock of the standard MUI pages. No payload, registry or launch.
Unicode true
ManifestDPIAware true
RequestExecutionLevel user
!include MUI2.nsh
!include "${PROJECT_ROOT}\installer\branding.nsh"
Name "ZAPRET (preview)"
OutFile "${PROJECT_ROOT}\artifacts\installer\nsis-preview.exe"
InstallDir "$TEMP\ZAPRET-preview"
BrandingText "Preview - no installation"
!define MUI_ICON "${PROJECT_ROOT}\src-tauri\icons\icon.ico"
!define MUI_HEADERIMAGE
!define MUI_HEADERIMAGE_BITMAP "${PROJECT_ROOT}\installer\assets\header.bmp"
!define MUI_WELCOMEFINISHPAGE_BITMAP "${PROJECT_ROOT}\installer\assets\sidebar.bmp"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_LANGUAGE "Russian"
!insertmacro MUI_LANGUAGE "English"
Section
  DetailPrint "Preview only - no files are installed."
SectionEnd
