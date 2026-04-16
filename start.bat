@echo off
:: Переходим в папку, где лежит сам bat-файл
cd /d "%~dp0"

:: Теперь запускаем команду
npm run tauri dev
pause