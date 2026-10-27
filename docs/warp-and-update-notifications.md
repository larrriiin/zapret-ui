# WARP routing, update notifications and Telegram preferences

## User behavior

WARP settings are available in the sidebar even before WARP is installed. Prepare site and executable lists at any time. Turning a routing switch on checks the official client, selects MASQUE and local proxy mode as needed, connects, verifies the loopback SOCKS5 handshake, then starts only the selected rules. Leaving a full-device tunnel requires confirmation, including through the home mode selector. Cancelling leaves the client untouched.

The switch reflects `runtime_active`. A saved `enabled` preference is only an instruction to restore a rule later. The adjacent status distinguishes **Off**, **Configured, not running**, **Connecting and starting**, **Active** and **Error**. A configured rule can be started again with its switch; **Do not restore these rules** clears its saved selection. Disabling one rule group keeps the other group and the WARP connection. Removing a group's last rule disables that group.

Adding or deleting entries saves the list immediately. Lists survive failures, mode changes, client removal and application restarts. The app never registers a WARP account or changes Zapret when preparing WARP routing.

**Restore routing rules**, in **Advanced settings**, defaults to off and saves immediately. It restores only groups whose saved selection is on, after startup or reconnect, when a connected MASQUE local proxy is verified. It does not switch modes or connect on its own. An incompatible or unavailable client produces an actionable error. Each group is attempted once per compatible connection; a failed driver/proxy start is not retried every polling tick. A manual routing action can retry.

All CLI operations, automatic recovery, probes and diagnostic collection share the backend `CLIENT` operation gate. Runtime teardown preserves desired selections. Windows PAC recovery still uses the durable ownership journal: it restores the previous proxy only if the current settings are still owned by this session. Automatic recovery does not continuously overwrite a proxy replaced by another application.

The separate connection card and **Check connection** button have been removed from WARP settings. Connect and disconnect remain on Home. The former `connect_on_start` preference is ignored when reading legacy settings and is no longer serialized: starting ZAPRET UI never connects WARP on its own. WARP is an independent service; quitting ZAPRET UI stops its rules but preserves the WARP connection.

## Advanced settings and limitations

The protocol picker shares the home-screen dropdown component, keyboard navigation and theme. Its only options are MASQUE and WireGuard. If the client has not reported its protocol, a selection placeholder and explanation appear without inventing a default or adding an Unknown option.

The home mode picker puts **Traffic and DNS (UDP)** first and **Local proxy** second, preserving the order of the remaining supported modes. Manual mode/protocol changes preserve an initially disconnected client: any implicit connection caused by the official configuration commands is cancelled, including after a failed configuration command. Reselecting the current compatible mode does nothing. A connected client is not disconnected by this safeguard. Explicitly activating routing rules still connects as described above.

The tunnel protocol is read from official-client settings (`settings.warp_tunnel_protocol` on the tested Windows client), with a strict text-settings fallback for older layouts. Only `MASQUE` and `WireGuard` are accepted. Protocol changes require a supported `tunnel protocol set --help` contract. Unsupported controls show an explanation. Selecting WireGuard while proxy rules are selected requires confirmation, disables both saved routing selections without deleting lists, and selects the full-device `warp` mode. The resulting protocol is read back rather than assumed.

The WARP **Check status** dialog includes the read-only connection check. It reports installation, actual connection/mode/protocol, a bounded SOCKS5 handshake where appropriate, and a bounded HTTPS request to Cloudflare's trace endpoint through the selected route. An HTTP success alone is insufficient: `warp=on` or `warp=plus` must be present. DNS-only modes are reported as unverified for traffic routing. This tests one endpoint at one moment; it does not certify that every configured site or application is reachable. Proxy/DNS/system routing settings are never changed by the check.

**Create diagnostic archive** requires an explicit click and privacy confirmation. Only a Cloudflare-signed `warp-diag.exe` beside the trusted installed `warp-cli.exe` is allowed, and its `--output` help contract is checked. Collection has a 90-second process timeout and produces a local ZIP in the app data `warp-diagnostics` folder. Reveal accepts no user-supplied path: it opens only the archive created during this session. No verbose capture flags or feedback uploads are added. Diagnostic archives may contain sensitive network information.

The old connections panel is removed. **View WARP traffic** opens the existing traffic monitor with its WARP filter and clears an unrelated search/process focus. It includes known internal relay flows by showing system processes. Existing backend exact-flow/interface classification is unchanged. Failed relay attempts remain diagnostic errors, not observed traffic or byte counts. Recent client and relay errors are available in the status dialog. **View WARP traffic** and **Create diagnostic archive** are equal-size buttons with icons on one row inside **Advanced settings**. Settings retain their information icons; the traffic action has no separate hint.

Official command references:

- [Cloudflare WARP CLI and protocol commands](https://developers.cloudflare.com/warp-client/get-started/linux/)
- [Cloudflare diagnostic collection](https://developers.cloudflare.com/cloudflare-one/team-and-resources/devices/cloudflare-one-client/troubleshooting/diagnostic-logs/)

## Updates

The existing signed UI updater, approved Flowseal channel and Telegram module check run on startup and every six hours. Concurrent checks share one request. Each component is checked independently; a failed endpoint cannot suppress an update from another component. Background checks never automatically use the project proxy or install updates. The manual button retains detailed results and explicit proxy retries.

An open window shows the existing modal containing all component updates. No in-app update toast is used. A hidden or minimized window receives a native notification through `tauri-plugin-notification` (minimum 2.7). Available components are grouped. Version identities are remembered in localStorage under `zapret_notified_updates_v1`, so the same versions do not repeatedly send Windows notifications across sessions. The automatic modal is shown once per version set in each app session, independently of earlier Windows notifications. Manual checks always retain their detailed dialog. A newly available version triggers another notification. A missing core remains actionable in the manual dialog but is not presented as an installed-component update.

Clicking the visible Windows notification or **View updates** action restores/focuses the main window and opens the existing update dialog. Version 2.7 reports Windows clicks only while the toast is on screen; clicks after it moves to Action Center are not guaranteed. The tray always provides **View updates**, including when a click arrives before the frontend listener is ready. Windows notification settings/Focus Assist can suppress display; the plugin cannot confirm OS delivery. Native interaction requires verification in an installed Windows build.

WARP update notifications were deliberately not added: the current integration has a trusted GA installer download, but no reviewed version-metadata/comparison contract for the installed Windows client. It does not infer versions from arbitrary pages or invent an update. No standalone notifications are created for internal helpers without independent release channels.

## Telegram autostart

The autostart switch calls `set_telegram_autostart(enabled)`. The backend operation gate reads the persisted configuration, changes only `start_with_zapret`, and atomically replaces the config file. It is allowed while the proxy runs and does not start, stop or restart it. Whole-form saves preserve the current backend autostart value rather than overwriting it from a stale form.

The frontend disables only the flag while saving and rolls it back on error. It never validates, saves, resets or clears the dirty state of draft host/port/DC/relay fields. **Save settings** remains necessary for those fields.

## Automated verification

```powershell
npm run build
npm run test:i18n
npm run test:traffic
npm run test:update-privacy
npm run test:update-notifications
npm run test:warp
node --test scripts/telegram-settings.test.mjs
npm run test:warp-ui
cargo test --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
```

The browser suite uses the existing Vite/IPC fixture and installed Edge in headless mode (`PLAYWRIGHT_CHANNEL` can select another installed Chromium channel). It tests the workflows in RU graphite at 1100×850 and EN light at 900×750, checks runtime errors and horizontal overflow, and exercises the real foreground update modal. Optional `QA_SCREENSHOTS_DIR` saves evidence outside the repository. Rust orchestration uses a fake client and never invokes the real CLI from ordinary tests. Preference tests use temporary directories; existing PAC/relay tests use isolated local sockets and recovery fixtures. Tests marked ignored remain opt-in.

Restricted Windows sandboxes can reject local test sockets or atomic rename in temporary directories; the complete Rust suite must then run in the normal development environment. Do not enable the real-client mode/port or WinDivert smoke tests on a live network merely to satisfy the ordinary suite.

## Manual Windows verification

Use a disposable Windows test environment for routing and proxy changes.

1. With WARP disconnected (and then absent), add/remove saved sites and executables; restart and verify the lists.
2. With the official client in a full-device mode, cancel activation and verify no mode/protocol change. Confirm activation and verify MASQUE + local proxy + only the requested group.
3. Start both groups. Turn each off independently. Confirm WARP stays connected and the other group still routes.
4. Test a refused connection, unavailable SOCKS endpoint and missing/blocked WinDivert driver. Verify actionable errors, inactive switches and retained lists. Retry manually.
5. Restart with **Restore routing rules** off and on, also with `--autostart`. Only previously selected groups should return when a compatible WARP proxy is already connected. Explicitly disabled groups must stay off. A legacy `connect_on_start: true` must never connect WARP. Reconnect through the official client and verify bounded recovery.
6. While disconnected, change modes in the app (including full tunnel to local proxy with WireGuard), reselect the same mode, and change protocol. Verify WARP remains disconnected. Repeat while connected and verify the connection remains enabled. Change modes/protocol externally; verify polling reflects reality and incompatible rules stop. Confirm/cancel WireGuard from the proxy and try an unsupported/managed client.
7. Start with an existing third-party Windows PAC/proxy. Verify exact restoration on exit/crash recovery, including when another app replaces the proxy during the session. Automation must not continuously reclaim that proxy.
8. Open the WARP **Check status** dialog in full tunnel, local proxy, DNS-only and offline modes. Verify route failures/unknown results are not shown as success.
9. Explicitly create/reveal a diagnostic ZIP. Verify privacy warning, bounded completion, signed-tool restrictions and that nothing is uploaded.
10. Follow **View WARP traffic** and verify the existing WARP filter, relay flows, metrics, other filters and log; failed connection attempts must not appear as observed byte counts.
11. Simulate one/multiple/new/same-version updates and one/all unreachable endpoints. Test visible, minimized, hidden and tray startup states. Confirm grouping/deduplication, dismissal, the native click/action, tray fallback and manual detailed checks. Never install automatically.
12. While TG WS Proxy is running, change autostart and verify no process restart. With invalid unsaved host/port/DC/relay drafts, toggle it, verify drafts/dirty state, then test a denied config write and rollback. Restart to verify the confirmed flag.
13. Repeat keyboard navigation and loading/error/disabled states in RU/EN, graphite/light, at 900×750 and the standard window size. Inspect expanded advanced settings as well as the default view.
