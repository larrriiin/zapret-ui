<p align="center">
  <img src="src-tauri/icons/128x128.png" width="96" alt="ZAPRET UI">
</p>

<h1 align="center">ZAPRET UI</h1>

<p align="center">
  Удобное управление <code>zapret</code> в Windows без ручной работы с BAT-файлами.<br>
  A modern Windows interface for managing <code>zapret</code> without working with BAT files manually.
</p>

<p align="center">
  <a href="https://github.com/larrriiin/zapret-ui/releases/latest"><strong>Скачать / Download</strong></a>
  ·
  <a href="https://github.com/larrriiin/zapret-ui/issues">Сообщить о проблеме / Report an issue</a>
</p>

<p align="center">
  <a href="https://github.com/larrriiin/zapret-ui/releases"><img alt="GitHub release" src="https://img.shields.io/github/v/release/larrriiin/zapret-ui?display_name=tag&sort=date"></a>
  <a href="https://github.com/larrriiin/zapret-ui/releases"><img alt="GitHub downloads" src="https://img.shields.io/github/downloads/larrriiin/zapret-ui/total"></a>
  <a href="https://github.com/larrriiin/zapret-ui/actions/workflows/checks.yml"><img alt="Version checks" src="https://github.com/larrriiin/zapret-ui/actions/workflows/checks.yml/badge.svg"></a>
  <a href="https://github.com/larrriiin/zapret-ui/actions/workflows/security.yml"><img alt="Security checks" src="https://github.com/larrriiin/zapret-ui/actions/workflows/security.yml/badge.svg"></a>
  <a href="https://github.com/larrriiin/zapret-ui/actions/workflows/releaser.yml"><img alt="Automatic release build" src="https://github.com/larrriiin/zapret-ui/actions/workflows/releaser.yml/badge.svg"></a>
</p>

<p align="center">
  <a href="#ru">Русский</a> · <a href="#en">English</a>
</p>

<p align="center">
  <img width="900" alt="Главное окно ZAPRET UI" src="https://github.com/user-attachments/assets/b7bcd009-2083-4c84-935c-8fc92640a2b3">
</p>

<!--
Рекомендуемая галерея для будущего обновления README:

1. docs/screenshots/home.png
   Главное окно: статус, выбранная стратегия и кнопка запуска.
2. docs/screenshots/lists.png
   Редактор пользовательских списков доменов и IP-адресов.
3. docs/screenshots/core-update.png
   Окно обновления ядра с версиями и источником Flowseal Stable.
4. docs/screenshots/rollback.png
   Настройки с доступной предыдущей версией и кнопкой отката.

После добавления файлов можно вставить сюда таблицу 2×2. Не используйте скриншоты
с личными путями, именами пользователей, IP-адресами или другими приватными данными.
-->

<a id="ru"></a>

## Русский

### Что это

**ZAPRET UI** — настольное приложение для Windows, которое предоставляет графический интерфейс для управления средствами обхода DPI из экосистемы [`zapret`](https://github.com/bol-van/zapret).

Приложение использует проверенные выпуски [`Flowseal/zapret-discord-youtube`](https://github.com/Flowseal/zapret-discord-youtube) как **поставщика ядра, готовых стратегий и сопутствующих файлов**. ZAPRET UI отвечает за удобное управление: установку и обновление ядра, запуск, работу со службой, выбор стратегии, пользовательские списки, сохранение настроек и откат.

> [!IMPORTANT]
> ZAPRET UI — не VPN и не средство анонимизации. Приложение не шифрует весь трафик и не скрывает ваш IP-адрес. Результат работы зависит от сети, провайдера, выбранной стратегии и актуальности ядра.

### Чем ZAPRET UI отличается от Flowseal

ZAPRET UI не конкурирует с Flowseal и не присваивает себе разработку стратегий. Это отдельный интерфейс и менеджер жизненного цикла поверх проверяемого ядра Flowseal.

| | Flowseal `zapret-discord-youtube` | ZAPRET UI |
|---|---|---|
| Основная роль | Готовая Windows-сборка `zapret`, стратегии, списки и служебные сценарии | Графическое приложение для установки, настройки, запуска и обновления ядра |
| Управление | BAT-файлы и консольные меню | Окна приложения, кнопки, настройки и системный трей |
| Стратегии | Создаются и публикуются в проекте Flowseal | Показываются в интерфейсе и запускаются без ручного редактирования BAT-файлов |
| Обновления | Механизмы и выпуски исходного проекта | Отдельные каналы обновления приложения и ядра |
| Выбор стабильного ядра | Актуальные выпуски Flowseal | В приложение попадает только версия, явно повышенная сопровождающим до канала `stable` |
| Целостность установки | Определяется исходным проектом | ZIP сверяется по SHA-256, проверяется во временной папке и активируется транзакционно |
| Откат | Зависит от ручного сценария пользователя | Предыдущую успешно установленную версию можно вернуть из интерфейса |
| Пользовательские данные | Хранятся в файлах сборки | Пользовательские списки и выбранные параметры сохраняются при обновлении ядра |

Иными словами: **Flowseal поставляет рабочее содержимое ядра, а ZAPRET UI делает его установку и ежедневное использование удобнее и безопаснее для обычного пользователя.**

### Возможности

- Запуск выбранной стратегии как службы Windows или временного процесса.
- Переключение между стратегиями Flowseal из графического интерфейса.
- Управление пользовательскими списками доменов, исключений, IP-адресов и подсетей.
- Настройка Game Filter, IPSet Filter и используемых fake-файлов.
- Автозапуск, системный трей, уведомления и выбор поведения при закрытии окна.
- Раздельные обновления интерфейса ZAPRET UI и ядра Flowseal.
- Проверка версии и источника стабильного ядра через управляемый канал.
- Скачивание ядра с проверкой SHA-256 до распаковки.
- Транзакционная установка: подготовка и проверка новой версии до замены рабочей.
- Откат к предыдущей успешно установленной версии.
- Сохранение пользовательских списков, выбранной стратегии, режима запуска и настроек фильтров при обновлении или откате.
- Доступ к вспомогательным функциям ядра, включая обновление IPSet и диагностику.

### Как устроено обновление ядра

Обновление интерфейса и обновление ядра — разные процессы:

1. Сопровождающий проверяет новый выпуск Flowseal.
2. Проверенная версия публикуется в [`core-channel/stable.json`](core-channel/stable.json) вместе с URL и SHA-256.
3. ZAPRET UI получает манифест стабильного канала. Если сеть или GitHub недоступны, используется встроенная в приложение резервная копия манифеста.
4. Архив скачивается во временную папку, сверяется по контрольной сумме и проверяется до активации.
5. Рабочая версия заменяется только после успешной проверки; предыдущая версия остаётся доступной для отката.
6. Если ядро было запущено, приложение восстанавливает выбранную стратегию и режим работы после успешной операции.

Новый выпуск ZAPRET UI для каждого обновления Flowseal **не требуется**: стабильное ядро продвигается отдельным изменением манифеста. Новая версия приложения нужна только при изменении самого интерфейса, логики или встроенного резервного манифеста.

### Требования

- Windows 10 или Windows 11, x64.
- Права администратора для установки драйвера, управления службой и выполнения сетевых операций.
- Доступ в интернет при первой установке ядра и для последующих обновлений.

Антивирус может реагировать на WinDivert как на сетевой инструмент повышенного риска. WinDivert необходим ядру для перехвата и фильтрации трафика. Скачивайте ZAPRET UI только из [официальных Releases](https://github.com/larrriiin/zapret-ui/releases) и не отключайте защиту системы без понимания причины предупреждения.

### Установка

1. Откройте страницу [Releases](https://github.com/larrriiin/zapret-ui/releases/latest).
2. Скачайте установщик из раздела **Assets**.
3. Запустите установщик и следуйте его инструкциям.
4. Откройте ZAPRET UI. При первом запуске приложение предложит установить стабильную версию ядра.
5. Выберите стратегию, запустите её во временном режиме для проверки, а затем при необходимости установите как службу.

При обновлении уже установленного приложения используйте встроенную проверку обновлений. Не скачивайте установщики и архивы из сторонних каналов, зеркал или сообщений от неизвестных пользователей.

### Важные ограничения

- Универсальной стратегии, одинаково работающей у всех провайдеров, не существует.
- Стратегия может перестать работать после изменений со стороны провайдера или сервиса.
- Успешный ping или открывшаяся главная страница не доказывают, что сервис работает полностью. Видео, превью, голосовые соединения и другие ресурсы могут загружаться с отдельных доменов, CDN и по другим протоколам.
- Если сайт открывается, но медиаконтент не работает, проверьте другую стратегию, пользовательские списки, Game Filter и IPSet Filter.
- Откат становится доступен только после того, как существует предыдущая успешно установленная версия ядра.

### Безопасность обновлений

ZAPRET UI:

- принимает только ожидаемый формат манифеста стабильного канала;
- использует HTTPS-ссылки на манифест и архив;
- проверяет SHA-256 скачанного архива;
- ограничивает размер получаемого манифеста;
- безопасно проверяет пути внутри ZIP-архива;
- валидирует обязательные файлы и версию до активации;
- не выполняет неявное понижение версии;
- восстанавливает прежнюю установку, если обновление не удалось.

Контрольная сумма подтверждает соответствие архива версии, одобренной в манифесте, но не заменяет аудит стороннего кода. Исходное ядро и стратегии публикуются проектом Flowseal и сохраняют собственные условия распространения.

### Частые вопросы

<details>
<summary><strong>Это отдельная реализация zapret?</strong></summary>

Нет. ZAPRET UI управляет ядром и стратегиями Flowseal через отдельный слой поставщика. Разработка самого `zapret` ведётся в проекте [`bol-van/zapret`](https://github.com/bol-van/zapret), а готовая Windows-сборка и стратегии поставляются [`Flowseal/zapret-discord-youtube`](https://github.com/Flowseal/zapret-discord-youtube).

</details>

<details>
<summary><strong>Почему приложение запрашивает права администратора?</strong></summary>

Они нужны для управления службой Windows, сетевым драйвером WinDivert и связанными системными операциями.

</details>

<details>
<summary><strong>Почему ZAPRET UI не устанавливает каждый новый выпуск Flowseal автоматически?</strong></summary>

Чтобы случайное или несовместимое upstream-обновление не попало пользователям без проверки. Сопровождающий сначала тестирует выпуск, затем явно переводит его в канал `stable`.

</details>

<details>
<summary><strong>Что произойдёт, если канал обновлений временно недоступен?</strong></summary>

Уже установленное ядро продолжит работать. Для определения стабильной версии приложение использует встроенный резервный манифест, если удалённый источник недоступен или вернул некорректные данные.

</details>

<details>
<summary><strong>Можно ли вернуться на предыдущую версию ядра?</strong></summary>

Да, после хотя бы одного успешного обновления. Откат выполняется транзакционно и сохраняет пользовательские списки и рабочие настройки.

</details>

### Разработка

Технологии проекта:

- [Tauri 2](https://tauri.app/) и Rust — нативная часть приложения;
- Vite, JavaScript и Tailwind CSS — интерфейс;
- GitHub Actions — проверки, сборка и публикация релизов.

Для разработки в Windows понадобятся Node.js 20 или новее, npm, Rust stable и системные компоненты сборки, необходимые Tauri.

```powershell
git clone https://github.com/larrriiin/zapret-ui.git
cd zapret-ui
npm ci
npm run tauri dev
```

Сборка установщика:

```powershell
npm ci
npm run tauri build
```

Проверки перед PR:

```powershell
npm ci
npm run build
npm run check-versions

cd src-tauri
cargo fmt --check
cargo test
cargo clippy --all-targets --no-deps -- -D warnings
```

Для локального аудита Rust-зависимостей:

```powershell
cargo install --locked cargo-audit
cd src-tauri
cargo audit
```

### Автоматическая сборка и выпуск

GitHub Actions выполняет:

- проверку синхронизации версии в `package.json`, `src-tauri/Cargo.toml` и `src-tauri/tauri.conf.json`;
- Rust-тесты на Windows;
- `clippy` для всех целей с запретом предупреждений;
- регулярный аудит Rust-зависимостей;
- автоматическую сборку Windows-релиза и артефактов встроенного обновления.

Чтобы выпустить новую версию ZAPRET UI:

```powershell
npm run set-version -- X.Y.Z
npm install --package-lock-only

cd src-tauri
cargo build
cd ..
```

После проверки изменений создайте и отправьте тег:

```powershell
git tag vX.Y.Z
git push origin vX.Y.Z
```

Тег `v*` запускает workflow [`publish`](.github/workflows/releaser.yml). GitHub автоматически собирает приложение на Windows и публикует GitHub Release с установщиком и подписанными артефактами обновления Tauri.

Чтобы перевести проверенный выпуск Flowseal в стабильный канал ядра:

```powershell
npm run promote-core-stable -- --version <FLOWSEAL_VERSION> --url <FLOWSEAL_ZIP_URL>
```

Скрипт скачивает архив, рассчитывает SHA-256 и обновляет [`core-channel/stable.json`](core-channel/stable.json). Перед коммитом обязательно проверьте diff, выполните полный набор тестов и вручную проверьте установку, запуск, обновление и откат.

### Структура проекта

```text
src/                         интерфейс приложения
src-tauri/src/core/          независимая логика канала, установки и отката ядра
src-tauri/src/providers/     адаптеры поставщиков ядра
src-tauri/src/providers/
  flowseal.rs                интеграция с Flowseal
core-channel/stable.json     одобренная стабильная версия ядра
scripts/                     проверка версий и служебные команды релиза
.github/workflows/           CI, проверки безопасности и публикация
```

### Как помочь проекту

- Описывайте воспроизводимые ошибки в [Issues](https://github.com/larrriiin/zapret-ui/issues).
- Указывайте версию ZAPRET UI, версию ядра, Windows, режим запуска и выбранную стратегию.
- Не публикуйте приватные домены, IP-адреса, журналы с персональными данными и другие секреты.
- Для изменений создавайте отдельную ветку и Pull Request.
- Перед отправкой PR выполните сборку, тесты, форматирование и `clippy`.

### Благодарности и лицензии

- [`bol-van/zapret`](https://github.com/bol-van/zapret) — оригинальный набор средств обхода DPI.
- [`Flowseal/zapret-discord-youtube`](https://github.com/Flowseal/zapret-discord-youtube) — поставщик Windows-ядра, стратегий и списков, используемых ZAPRET UI.
- [Tauri](https://tauri.app/) и другие open-source проекты, на которых построено приложение.

Исходный код ZAPRET UI распространяется по лицензии [MIT](LICENSE). Загружаемые сторонние компоненты, бинарные файлы, стратегии и списки сохраняют авторство и лицензии соответствующих проектов. ZAPRET UI не является официальным клиентом Flowseal или `bol-van/zapret`.

Используйте приложение только в соответствии с законодательством вашей страны, правилами вашей сети и условиями используемых сервисов. Авторы и участники проекта не гарантируют работоспособность конкретной стратегии в любой сети.

---

<a id="en"></a>

## English

### What is ZAPRET UI?

**ZAPRET UI** is a Windows desktop application that provides a graphical interface for managing DPI-circumvention tools from the [`zapret`](https://github.com/bol-van/zapret) ecosystem.

The application uses tested releases of [`Flowseal/zapret-discord-youtube`](https://github.com/Flowseal/zapret-discord-youtube) as a **provider of the core, ready-made strategies, and supporting files**. ZAPRET UI handles the user-facing lifecycle: installation and updates, service management, strategy selection, user lists, settings preservation, and rollback.

> [!IMPORTANT]
> ZAPRET UI is not a VPN or an anonymity tool. It does not encrypt all traffic or hide your IP address. Results depend on your network, ISP, selected strategy, and core version.

### How is it different from Flowseal?

ZAPRET UI does not compete with Flowseal and does not claim ownership of its strategies. It is a separate graphical interface and lifecycle manager built around an approved Flowseal core.

| | Flowseal `zapret-discord-youtube` | ZAPRET UI |
|---|---|---|
| Primary role | Ready-to-use Windows distribution of `zapret`, strategies, lists, and maintenance scripts | Graphical application for installing, configuring, running, and updating the core |
| Interaction | BAT files and console menus | Application windows, controls, settings, and system tray |
| Strategies | Developed and published by Flowseal | Displayed and launched through the UI without editing BAT files manually |
| Updates | Upstream releases and update mechanisms | Separate application and core update channels |
| Stable core selection | Current Flowseal releases | Only a release explicitly promoted by the maintainer enters the `stable` channel |
| Installation integrity | Defined by the upstream project | ZIP is SHA-256 verified, validated in staging, and transactionally activated |
| Rollback | Depends on the user's manual workflow | The previous successfully installed core can be restored from the UI |
| User data | Stored in distribution files | User lists and selected settings are preserved across core updates |

In short: **Flowseal supplies the working core content; ZAPRET UI makes it easier and safer for regular users to install and operate it.**

### Features

- Run the selected strategy as a Windows service or a temporary process.
- Switch between Flowseal strategies from a graphical interface.
- Edit user domain, exclusion, IP address, and subnet lists.
- Configure Game Filter, IPSet Filter, and active fake files.
- Autostart, system tray integration, notifications, and configurable close behavior.
- Independent updates for the ZAPRET UI application and the Flowseal core.
- A maintainer-controlled stable core channel with explicit version and source metadata.
- SHA-256 verification before a downloaded core is extracted.
- Transactional installation: stage and validate a new version before replacing the working one.
- Rollback to the previous successfully installed core.
- Preserve user lists, the selected strategy, run mode, and filter settings during update or rollback.
- Access supporting core actions such as IPSet updates and diagnostics.

### How core updates work

Application updates and core updates are separate:

1. A maintainer tests a new Flowseal release.
2. The approved version is published in [`core-channel/stable.json`](core-channel/stable.json) together with its URL and SHA-256.
3. ZAPRET UI loads the stable channel manifest. If the network or GitHub is unavailable, it uses a manifest embedded in the application as a fallback.
4. The archive is downloaded to a temporary directory, checksum-verified, and validated before activation.
5. The working version is replaced only after validation succeeds; the previous version remains available for rollback.
6. If the core was running, the application restores the selected strategy and run mode after a successful operation.

A new ZAPRET UI release is **not required** for every Flowseal update: the stable core is promoted through the separate channel manifest. A new application build is needed only when the UI, application logic, or embedded fallback manifest changes.

### Requirements

- Windows 10 or Windows 11, x64.
- Administrator privileges for driver installation, Windows service management, and related network operations.
- Internet access for the initial core installation and future updates.

Antivirus software may flag WinDivert as a risk-oriented networking tool. The core requires WinDivert to intercept and filter traffic. Download ZAPRET UI only from the official [Releases](https://github.com/larrriiin/zapret-ui/releases) page, and do not disable system protection unless you understand the warning.

### Installation

1. Open the latest [Release](https://github.com/larrriiin/zapret-ui/releases/latest).
2. Download the installer from **Assets**.
3. Run the installer and follow its prompts.
4. Launch ZAPRET UI. On first launch, the application will offer to install the stable core.
5. Select a strategy, test it in temporary mode, and install it as a service if needed.

For an existing installation, use the built-in application updater. Do not download installers or core archives from third-party channels, mirrors, or messages from unknown users.

### Important limitations

- There is no universal strategy that works for every ISP and network.
- A working strategy may stop working after changes made by an ISP or online service.
- A successful ping or a loaded landing page does not prove that a service works completely. Video, thumbnails, voice connections, and other resources may use different domains, CDNs, and protocols.
- If a website loads but media does not, try another strategy and review the user lists, Game Filter, and IPSet Filter.
- Rollback becomes available only after a previous core version has been installed successfully.

### Update safety

ZAPRET UI:

- accepts only the expected stable-channel manifest format;
- requires HTTPS manifest and artifact URLs;
- verifies the downloaded archive using SHA-256;
- limits the size of the downloaded manifest;
- validates paths inside ZIP archives;
- checks required files and version metadata before activation;
- does not perform an implicit downgrade;
- restores the previous installation if an update fails.

The checksum confirms that the archive matches the version approved in the manifest; it is not a substitute for auditing third-party code. The core and strategies are published by Flowseal and retain their own distribution terms.

### FAQ

<details>
<summary><strong>Is this a separate implementation of zapret?</strong></summary>

No. ZAPRET UI manages the Flowseal core and strategies through a dedicated provider layer. The underlying `zapret` project is maintained at [`bol-van/zapret`](https://github.com/bol-van/zapret), while the ready-to-use Windows distribution and strategies come from [`Flowseal/zapret-discord-youtube`](https://github.com/Flowseal/zapret-discord-youtube).

</details>

<details>
<summary><strong>Why does the application require administrator privileges?</strong></summary>

They are required to manage the Windows service, the WinDivert network driver, and related system operations.

</details>

<details>
<summary><strong>Why does ZAPRET UI not install every new Flowseal release immediately?</strong></summary>

This prevents an untested or incompatible upstream update from reaching users automatically. A maintainer tests the release first and explicitly promotes it to the `stable` channel.

</details>

<details>
<summary><strong>What happens when the update channel is temporarily unavailable?</strong></summary>

The installed core continues to work. When the remote source is unavailable or invalid, the application uses its embedded fallback manifest to determine the stable version.

</details>

<details>
<summary><strong>Can I restore the previous core version?</strong></summary>

Yes, after at least one successful update. Rollback is transactional and preserves user lists and runtime settings.

</details>

### Development

Project stack:

- [Tauri 2](https://tauri.app/) and Rust for the native application;
- Vite, JavaScript, and Tailwind CSS for the frontend;
- GitHub Actions for checks, builds, and releases.

Development on Windows requires Node.js 20 or newer, npm, Rust stable, and the system build components required by Tauri.

```powershell
git clone https://github.com/larrriiin/zapret-ui.git
cd zapret-ui
npm ci
npm run tauri dev
```

Build the installer:

```powershell
npm ci
npm run tauri build
```

Run checks before opening a PR:

```powershell
npm ci
npm run build
npm run check-versions

cd src-tauri
cargo fmt --check
cargo test
cargo clippy --all-targets --no-deps -- -D warnings
```

For a local Rust dependency audit:

```powershell
cargo install --locked cargo-audit
cd src-tauri
cargo audit
```

### Automated builds and releases

GitHub Actions performs:

- version synchronization checks across `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`;
- Rust tests on Windows;
- all-target `clippy` with warnings treated as errors;
- scheduled Rust dependency audits;
- automatic Windows release builds and updater artifact publication.

To release a new ZAPRET UI version:

```powershell
npm run set-version -- X.Y.Z
npm install --package-lock-only

cd src-tauri
cargo build
cd ..
```

After reviewing and testing the changes, create and push a tag:

```powershell
git tag vX.Y.Z
git push origin vX.Y.Z
```

Any `v*` tag triggers the [`publish`](.github/workflows/releaser.yml) workflow. GitHub automatically builds the application on Windows and publishes a GitHub Release containing the installer and signed Tauri updater artifacts.

To promote a tested Flowseal release to the stable core channel:

```powershell
npm run promote-core-stable -- --version <FLOWSEAL_VERSION> --url <FLOWSEAL_ZIP_URL>
```

The script downloads the archive, calculates its SHA-256, and updates [`core-channel/stable.json`](core-channel/stable.json). Review the diff, run the complete test suite, and manually test installation, startup, update, and rollback before committing it.

### Project layout

```text
src/                         application frontend
src-tauri/src/core/          provider-neutral core channel, installation, and rollback
src-tauri/src/providers/     core provider adapters
src-tauri/src/providers/
  flowseal.rs                Flowseal integration
core-channel/stable.json     approved stable core version
scripts/                     version checks and release helpers
.github/workflows/           CI, security checks, and publishing
```

### Contributing

- Report reproducible bugs through [Issues](https://github.com/larrriiin/zapret-ui/issues).
- Include the ZAPRET UI version, core version, Windows version, run mode, and selected strategy.
- Do not publish private domains, IP addresses, logs containing personal data, or secrets.
- Use a dedicated branch and Pull Request for code changes.
- Run the build, tests, formatter, and `clippy` before submitting a PR.

### Credits and licensing

- [`bol-van/zapret`](https://github.com/bol-van/zapret) — the original DPI-circumvention toolkit.
- [`Flowseal/zapret-discord-youtube`](https://github.com/Flowseal/zapret-discord-youtube) — the Windows core, strategies, and lists consumed by ZAPRET UI.
- [Tauri](https://tauri.app/) and the other open-source projects used to build the application.

ZAPRET UI source code is distributed under the [MIT License](LICENSE). Downloaded third-party components, binaries, strategies, and lists retain the copyright and licenses of their respective projects. ZAPRET UI is not an official client of Flowseal or `bol-van/zapret`.

Use the application only in accordance with the laws of your country, your network policies, and the terms of the services you access. The maintainers and contributors do not guarantee that a particular strategy will work on every network.
