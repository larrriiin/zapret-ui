# Application automations

The lightning icon opens application rules. Enable observation, add an `.exe`, select actions independently for Zapret and WARP, then save. Rules are stored in `application-rules.json` in the application's data directory. Observation is off by default.

- Actions: leave connection unchanged, enable/connect, disable/disconnect; optionally select a Zapret strategy or WARP mode.
- On close: restore previous state, keep changes, or turn off the services controlled by that rule.
- The top enabled running rule wins independently for each provider. Drag the left handle up or down to reorder rules (or use Up/Down/Home/End on the focused handle). Escape cancels dragging; failed saves restore the previous order. When it ends, the next applicable rule takes over; the original baseline remains available for restoration.
- A manual state change pauses that provider until all applicable rules end. A failed action also pauses it instead of repeatedly reconnecting. The section shows errors and pause status.
- Matching uses the full executable path, case insensitive. Multiple processes with the same path keep the rule active until the last exits. Native Windows process snapshots are sampled every two seconds, with a four-second grace period for restarts. Detection of closure normally takes four to six seconds; slow connection operations can extend this.
- Runs in the Rust backend while the webview is minimized. Fully quitting stops observation and leaves the current connection state. An already running application is detected when observation starts.
- Zapret strategy-only actions require Zapret to be running. To start a stopped service with a particular strategy, select Enable and the strategy. Existing service/temporary launch mode is restored when available; new starts use service mode.
- WARP Local proxy rules connect the official client, wait for a verified compatible SOCKS proxy, then activate already enabled site/application routing, even if global automatic routing restoration is off. Disabled selections remain disabled. Disconnecting or switching away from proxy releases runtime routes while preserving lists and enabled selections. Returning to connected proxy mode reactivates them. Saving a rule authorizes its mode and restoration. Unsupported modes, connection and routing failures are reported. Proxy readiness and connection transitions use bounded waits.
- Monitoring does not launch target applications or intercept their first connections. Applications may begin network activity before a rule has completed.

Validation: `npm run test:automations` runs browser fixtures without changing system networking. `cargo test --manifest-path src-tauri/Cargo.toml automations::tests --lib` covers state restoration, overlapping rules, manual overrides, failures, validation and native process enumeration. Actual WARP/Zapret connections require a separate installed-client smoke test.

Each rule can use one of twelve bundled Material Symbols. Existing rules without an icon use the application symbol. Edit and delete are available in the three-dot action menu; keyboard arrows and Escape are supported. Changing only the name or icon does not reapply connection actions.

For Local proxy mode, rules may either use normal WARP settings (the default for existing rules) or specify a subset of saved sites and executable paths. Rule-specific routes run without rewriting saved WARP lists or enabled switches. Returning to a previous rule or restoring the baseline restores its routing selection. Explicit selections can include saved entries whose global toggle is off; selecting an entry in a rule authorizes its temporary routing. Entries removed from WARP settings are rejected at activation rather than silently replaced. Runtime routing overrides are discarded on full application exit.

The Add rule action and each rule's Edit action open a native modal editor. There is no collapsible inline editor. Save commits the rule; Cancel, the close button, Escape or a backdrop click discard the draft. Validation and save failures are displayed inside the dialog, which stays open. During a save, closing is temporarily disabled.
