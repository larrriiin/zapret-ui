# Optional Telegram module

Telegram is optional during Preparation in first-time setup. Both additional
components default to unchecked. Installed components show a check mark and
installation status instead of a disabled checkbox. Missing WARP uses the
existing official, publisher-verified MSI installer. No tunnel is connected
automatically. Detection failures offer retry
and do not prevent finishing setup.

Settings always contains Telegram installation controls. The sidebar entry is
hidden until native installation validation succeeds, including after restart.
Removing the module stops its owned process and hides the entry again. Small
configuration and rotating log files remain, preserving the connection secret
if the module is reinstalled. Nothing is installed into the system Python.

## Distribution

The initial module is Flowseal **1.11.2**, Python **3.13.12** (Windows x64), and
thirteen binary Python wheels, including `httpx[http2]` and its dependencies.
`src-tauri/telegram-module.json` pins all fifteen URLs,
byte sizes, and SHA-256 values. Downloads total **17,490,647 bytes**. No archives
or Python runtime are bundled into the application installer.

Downloads go directly to Python.org, GitHub and Python's package CDN over HTTPS.
Redirect hosts are restricted. Every archive is size/hash checked before
extraction, with traversal/symlink checks and an expanded size limit. Staging is
promoted only after the isolated interpreter imports crypto and certificate
dependencies successfully. Failed staging is discarded and can be retried.
The source archive contributes only `proxy/` and its MIT license. Python and
wheel license files are retained. The upstream GUI/tray and pip are not used.

The small `telegram-runner.py` adapter starts unmodified upstream proxy code,
defaults to loopback, redacts the secret in logs and watches the parent process.
It terminates if the app exits/crashes, including during a startup race. Normal
exit stops and waits for the child; closing to tray leaves it running. The module
does not auto-start on the next application launch. Direct Telegram WebSocket
and TCP fallback are used. Settings expose the bind IPv4 address, port, secret
regeneration, DC-to-IPv4 overrides, Cloudflare Proxy and custom relay domains,
and Cloudflare Worker domains. Cloudflare Proxy defaults to enabled, matching
upstream and making its HTTP/2 media fallback available. Older configurations
without a `cfproxy` field inherit this default; an explicitly saved `false` is
preserved. Enabling Cloudflare Proxy also enables upstream domain refresh; Worker routes can be
enabled independently. Changes require the proxy to be stopped. Secret rotation
preserves other configuration but invalidates existing connection links.
The Restart button stops and waits for the owned process, then starts a new one
with the saved settings and secret. Startup logs include the relay switches and
DC overrides, without the secret. Running status reports process/listener state;
it does not prove that every Telegram datacenter is reachable from this network.

## Module updates

Installed modules are checked at application startup and when opening the update
dialog. No Telegram release request is made if the module is absent. The global
dialog includes a separate TG WS Proxy row, version, update button, progress and
retry for failed checks. Installation of an update requires clicking its button.

The updater resolves the latest stable official GitHub release redirect to a validated
version tag and downloads its source over HTTPS. It records the source tag and SHA-256;
this is not a separately signed update manifest. Only upstream proxy source and
license are replaced. The pinned Python runtime is retained. Missing or changed
reviewed wheels from the application's lock are downloaded with size/hash checks
and extracted only in staging. This upgrades existing 1.10.2 installations with
the HTTP/2 dependencies required by 1.11.2; reinstalling the module is unnecessary.
The installed metadata records these dependencies to avoid downloading them again.
An isolated import/API compatibility check runs in staging before activation.
Incompatible upstream releases require an application update to revise the
adapter or dependencies; import checks cannot guarantee all runtime behavior of
future releases.

Configuration is retained outside the module directory. A running proxy is
stopped for activation and restarted afterward. Failed startup restores the
previous module and attempts to restart it. A previous successful version remains
on disk for recovery (approximately another 37 MB); removal deletes both copies.
An interrupted directory swap can recover the previous module on the next status
check. Existing installations survive application updates.

`scripts/lock-telegram-module.py` generates the lock from previously downloaded
archives and checks wheel hashes against PyPI metadata. It is a maintainer tool;
users never run it. Update its fixed upstream/Python versions and download the
matching source, embedded runtime, and wheels into clean staging directories
before regenerating. Review all dependency changes before committing.

## Verification

- `cargo test --manifest-path src-tauri/Cargo.toml telegram::`
- `cargo test --manifest-path src-tauri/Cargo.toml upgrades_legacy_module_in_staging -- --ignored`
  (opt-in Windows regression test; requires the locked archives plus the original
  v1.10.2 source archive at `artifacts/tg-source/legacy-source.zip`). Verifies the
  original import failure, dependency migration through the native staging code,
  activation, preservation of the previous module and unchanged configuration.
- `node --test scripts/i18n.test.mjs scripts/first-run-state.test.mjs scripts/telegram-settings.test.mjs scripts/update-check-privacy.test.mjs`
- `node node_modules/vite/bin/vite.js build`
- `python scripts/test-telegram-module.py` (opt-in Windows test; requires the
  already downloaded locked archives under `artifacts/tg-downloads` and
  `artifacts/tg-source/source.zip`). Uses isolated `artifacts/tg-smoke`, a free
  loopback port, verifies crypto, startup, secret redaction, configuration mapping,
  secret regeneration and persistent config. It also runs offline routing tests
  for the default fallback, HTTP/2 media, explicitly disabled CF and Worker routing.
- `node scripts/preview-telegram.cjs` generates an isolated browser fixture for
  the Vite dev server. All native commands are mocked. Query parameters `setup=1`,
  `installed=1`, `warp=1`, `update=1`, and `fail=1` exercise setup, installed
  components, module updates and a failed-first-download retry. This fixture is
  outside the production bundle.

Actual Telegram account traffic and media loading require a real Telegram client
and network-specific manual testing. WARP installation is not run as a test on
the developer's machine.
