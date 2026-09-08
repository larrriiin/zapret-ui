# Optional Telegram module

Telegram is optional during Preparation in first-time setup. Both additional
components default to unchecked. WARP is offered only after local detection
confirms it is absent; its existing official, publisher-verified MSI installer
is reused. No tunnel is connected automatically. Detection failures offer retry
and do not prevent finishing setup.

Settings always contains Telegram installation controls. The sidebar entry is
hidden until native installation validation succeeds, including after restart.
Removing the module stops its owned process and hides the entry again. Small
configuration and rotating log files remain, preserving the connection secret
if the module is reinstalled. Nothing is installed into the system Python.

## Distribution

The initial module is Flowseal **1.10.2**, Python **3.13.12** (Windows x64), and
five binary Python wheels. `src-tauri/telegram-module.json` pins all seven URLs,
byte sizes, and SHA-256 values. Downloads total **16,923,850 bytes**; the tested
module occupies approximately **33.2 MB** after extraction/imports. No archives
or Python runtime are bundled into the application installer.

Downloads go directly to Python.org, GitHub and Python's package CDN over HTTPS.
Redirect hosts are restricted. Every archive is size/hash checked before
extraction, with traversal/symlink checks and an expanded size limit. Staging is
promoted only after the isolated interpreter imports crypto and certificate
dependencies successfully. Failed staging is discarded and can be retried.
The source archive contributes only `proxy/` and its MIT license. Python and
wheel license files are retained. The upstream GUI/tray and pip are not used.

The small `telegram-runner.py` adapter starts unmodified upstream proxy code,
binds only loopback, redacts the secret in logs and watches the parent process.
It terminates if the app exits/crashes, including during a startup race. Normal
exit stops and waits for the child; closing to tray leaves it running. The module
does not auto-start on the next application launch. Direct Telegram WebSocket
and TCP fallback are used; the upstream shared Cloudflare relay fallback and
its background domain-list refresh are disabled.

## Updating the reviewed version

This first implementation uses a lock embedded in the application, **not an
automatic upstream update channel**. It deliberately does not download arbitrary
latest releases. Existing installations are retained across application updates.
To ship a new module, update/review the lock and adapter, test compatibility and
implement version-aware transactional replacement (retaining config/rollback)
before enabling automatic module updates. A separately signed channel can follow.

`scripts/lock-telegram-module.py` generates the lock from previously downloaded
archives and checks wheel hashes against PyPI metadata. It is a maintainer tool;
users never run it. Update its fixed upstream/Python versions and download the
matching source, embedded runtime, and wheels into clean staging directories
before regenerating. Review all dependency changes before committing.

## Verification

- `cargo test --manifest-path src-tauri/Cargo.toml telegram::tests`
- `node --test scripts/i18n.test.mjs scripts/first-run-state.test.mjs`
- `node node_modules/vite/bin/vite.js build`
- `python scripts/test-telegram-module.py` (opt-in Windows test; requires the
  already downloaded locked archives under `artifacts/tg-downloads` and
  `artifacts/tg-source/source.zip`). Uses isolated `artifacts/tg-smoke`, a free
  loopback port, verifies crypto, startup, secret redaction and persistent config.
- `node scripts/preview-telegram.cjs` generates an isolated browser fixture for
  the Vite dev server. All native commands are mocked. Query parameters `setup=1`,
  `installed=1`, `warp=1`, and `fail=1` exercise setup, installed components and
  a failed-first-download retry. This fixture is outside the production bundle.

Actual Telegram account traffic and media loading require a real Telegram client
and network-specific manual testing. WARP installation is not run as a test on
the developer's machine.
