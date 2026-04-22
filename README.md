# Zapret UI

[Русский](#russian) | [English](#english)

<a name="russian"></a>
## Описание (RU)
**Zapret UI** — это современный графический интерфейс для утилиты `zapret`, предназначенной для обхода систем анализа трафика (DPI). Приложение упрощает настройку и управление службой, позволяя легко переключать стратегии, редактировать списки доменов и следить за состоянием сервиса.

### Основные возможности
- **Управление службой**: Запуск zapret как полноценного Windows-сервиса или в режиме временного процесса.
- **Гибкие стратегии**: Быстрое переключение между готовыми пресетами обхода.
- **Управление списками**: Удобный интерфейс для редактирования белых и черных списков доменов и IP-адресов.
- **Автоматизация**: Встроенная проверка обновлений ядра и графического интерфейса.
- **Диагностика**: Инструментарий для проверки интернет-соединения и очистки кэша сервисов (например, Discord).

### Установка
1. Скачайте последнюю версию со страницы [Releases](https://github.com/larrriiin/zapret-ui/releases).
2. Запустите инсталлятор и следуйте инструкциям.
3. При первом запуске приложение предложит загрузить необходимые бинарные файлы ядра.

### Выпуск новой версии
Версия хранится в трёх файлах и должна совпадать во всех. Чтобы не править их руками:

```
npm run set-version 2026.6.1
```

Скрипт обновит `package.json`, `src-tauri/Cargo.toml` и `src-tauri/tauri.conf.json`. Файл `version.txt` в корне трогать не нужно — `src-tauri/build.rs` перезаписывает его из `tauri.conf.json` при каждой сборке. CI-проверка `checks` на каждом PR падает, если версии разъехались.

После `npm run set-version` остаётся закоммитить изменения и поставить git-тег `vX.Y.Z` — workflow `publish` соберёт и выложит релиз.

---

<a name="english"></a>
## Description (EN)
**Zapret UI** is a modern graphical interface for the `zapret` utility, designed to bypass Deep Packet Inspection (DPI) systems. This application simplifies configuration and service management, allowing you to easily switch strategies, manage domain lists, and monitor the service status.

### Key Features
- **Service Management**: Run zapret as a full Windows service or as a temporary process.
- **Flexible Strategies**: Quickly switch between pre-defined bypass presets.
- **List Management**: User-friendly interface for editing domain/IP allowlists and blocklists.
- **Automation**: Built-in update checks for both the core engine and the UI.
- **Diagnostics**: Built-in tools for testing connection and clearing service caches (e.g., Discord).

### Installation
1. Download the latest version from the [Releases](https://github.com/larrriiin/zapret-ui/releases) page.
2. Run the installer and follow the prompts.
3. On the first launch, the app will offer to download the necessary core binaries.

### Cutting a new release
The app version lives in three files and must match across all of them. Use the helper script instead of editing each one:

```
npm run set-version 2026.6.1
```

This updates `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`. Do not edit `version.txt` — `src-tauri/build.rs` rewrites it from `tauri.conf.json` on every build. A CI `checks` job runs on every PR and fails if the three files drift.

After `npm run set-version`, commit the changes and push a `vX.Y.Z` tag to trigger the `publish` workflow.

## License
MIT
