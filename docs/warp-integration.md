# Cloudflare WARP integration

## Architecture

- `src-tauri/src/providers/warp.rs`: concrete Tauri commands for status, connect,
  disconnect, mode and proxy port. It never calls Zapret, changes its service, or
  persists a separate WARP connection preference. `lib.rs` only registers commands.
- Discovery checks `ProgramW6432`, `ProgramFiles` and `ProgramFiles(x86)` under
  `Cloudflare/Cloudflare WARP/warp-cli.exe`; PATH and frontend executable paths are
  not accepted. Authenticode must validate with publisher `Cloudflare, Inc.`.
- CLI capabilities are cached against executable path/modification time. Available
  modes come from `mode --help`; SOCKS5 and port support also come from CLI help.
  Unknown formats fail visibly instead of guessing commands or claiming a connection.
- A process-wide mutex prevents concurrent CLI calls across webviews. Work runs on
  blocking workers, with drained stdout/stderr, hidden console windows and a 10-second
  CLI timeout. No generic command execution IPC is exposed.
- `src/features/warp.js` and `src/components/warp.html`: independent controls and a
  three-second polling loop. A user operation waits for a pending poll. The backend
  remains the source of truth for installation, connection, mode and proxy endpoint.
- The original home HTML is unchanged. The installed state applies scoped CSS and
  moves the existing status heading into the Zapret card. Existing strategy and
  connection controls retain their nodes and listeners. Removing WARP restores the
  original composition. IPSet, game filters and advanced settings stay below it.
- Installation is offered only in Settings. New UI strings use the existing RU/EN
  dictionaries; WARP+ is never treated as a network mode.
- `warp-mode.js` provides a keyboard-accessible popup styled like the Zapret
  strategy picker. It is mounted outside the card to avoid header clipping.
- Both WARP status buttons open a report with loading, failure and client details.
  The Settings row is placed after general application preferences and shows the
  live connection state. Installation remains available when the client is absent.
- `connection-summary.js` combines the independently polled providers in the
  header: strategy, strategy + WARP, WARP alone, or disconnected. Zapret's card
  and controlled restart state retain their own behavior.

## Verified CLI contract

Inspected on Windows with `warp-cli 2026.7.1343.0`:

| CLI mode | UI meaning |
| --- | --- |
| `doh` | DNS only (HTTPS) |
| `dot` | DNS only (TLS) |
| `warp` | Traffic and DNS (UDP) |
| `warp+dot` | Traffic and DNS (TLS) |
| `warp+doh` | Traffic and DNS (HTTPS) |
| `proxy` | Local SOCKS5 proxy |
| `tunnel_only` | Traffic only |

`--json status` supplies `status`; `--json settings` supplies
`settings.operation_mode` and, in proxy mode, `settings.proxy_port`.
`proxy port <PORT>` changes the port. The help specifies loopback `127.0.0.1`.
No port or connection is fabricated if the client cannot report it. Local proxy
activity reflects the client connection state, not an independent end-to-end probe.

## Selected websites

Expand **Websites through WARP** on the WARP card. Enter one domain per line,
select **Local proxy**, connect WARP, then choose **Enable for websites**.
The list is saved locally; enabling also saves the edited list. Domains include
their subdomains. Unicode domains are converted to IDNA; URLs, IPs, ports,
invalid labels, and lists over 500 entries are rejected by the backend.

`providers/warp/sites.rs` serves an in-memory PAC script on a random loopback
port with an exact, revision-specific URL. Selected hosts return only the
verified WARP SOCKS5 endpoint; all other hosts return `DIRECT`. A domain such as
`example.com` never matches `notexample.com` or `example.com.other.test`.
The service accepts bounded HTTP GET requests, requires a matching Host header,
and exposes no filesystem or configuration-write endpoint.

Activation uses WinINet's per-connection API for the current user's default
connection, temporarily enabling the PAC URL and disabling manual proxy and
auto-detection flags. Manual proxy server/bypass values are not rewritten.
The previous flags and PAC URL are flushed to `warp-sites-recovery.json` in
the app data directory before the Windows settings change. Restoration only
runs if the current flags and URL still match the settings owned by ZAPRET UI.
External proxy changes are preserved. Failed restoration retains the journal
for retry. Startup recovery precedes loading domain preferences.

Rules are session-scoped and disabled on disconnect, mode change, app exit,
or a detected external proxy/port/client change. The app may remain in the tray.
After a crash, the next startup restores owned settings; browser behavior while
the app is absent depends on its PAC cache/fallback. This is selective browser
routing, not a fail-closed firewall. Preferences persist in `warp-sites.json`;
activation does not automatically resume on startup.

Scope: browsers using Windows system proxy settings (for example Chrome/Edge).
Browser policies, extensions, or custom proxy settings can override this.
Reload tabs after changes; existing connections can remain on their old route.
CDN/authentication domains must be listed separately when needed. Games, UDP,
WebRTC, and programs ignoring system proxy settings are outside this feature.
The mode does not change WARP registration, install a driver, or use Zero Trust.

Tests execute the generated PAC in Node.js, exercise the local HTTP endpoint,
validate domains/IDNA, and check recovery ownership and retry behavior with
injected settings. A WinINet read-only check runs on Windows. The UI harness
checks loading/saving/enabling/disabling rules, invalid edits, and dirty-input
preservation. Live WARP/browser routing and actual Windows proxy mutation still
require a native smoke test; the default tests do not change the system proxy.

## Installer trust boundary

`src-tauri/src/providers/warp/installer.rs` downloads the MSI directly from
`https://downloads.cloudflareclient.com/v1/download/windows/ga`, linked by
[Cloudflare's Windows setup documentation](https://developers.cloudflare.com/warp-client/get-started/windows/).
HTTPS redirects are limited to the exact official download hosts. Downloads have
time/size limits and exclusive temporary files. The downloaded MSI is reopened
read-only with write/delete sharing denied, verified with Authenticode and the
expected Cloudflare publisher, and held through `msiexec /i ... /norestart`.
The official installer UI handles installation; no silent TOS acceptance,
registration, bundling, mirroring or driver redistribution is added. MSI success
code 3010 is accepted without forcing a reboot. Temporary files are cleaned up
after completion/failure, and detection continues during and after installation.
First-time onboarding/registration and any organization policy remain in the
official client. CLI failures explain that recovery path.

## Validation

```powershell
node node_modules/vite/bin/vite.js build
node --test scripts/i18n.test.mjs
cargo test --manifest-path src-tauri/Cargo.toml --offline
```

Opt-in integration tests (require an installed official client):

```powershell
cargo test --manifest-path src-tauri/Cargo.toml installed_client_status -- --ignored
# Requires WARP disconnected; temporarily changes modes and proxy port, restores both.
cargo test --manifest-path src-tauri/Cargo.toml installed_client_modes_and_proxy -- --ignored
```

`node scripts/preview-warp.mjs` serves a browser regression harness at
`http://127.0.0.1:1420/warp-preview.html`. It uses mocked IPC and displays results
below the home page. Checks include exact original element bounds without WARP
and after removal, installation detection, independent providers, double clicks,
mode/port changes, failures, external synchronization, serialization and RU/EN.
Use `?lang=en&theme=light&proxy=1` for the proxy/light/English presentation.
Validate the native default 1100×980 and minimum 900×750 window sizes.

Live connect/disconnect routing and an actual MSI installation need a Windows
network/installer smoke test. The development checks do not reinstall or upgrade
the user's existing client.
