# Strategy catalog

Zapret UI uses `binaries/strategies.json` as the runtime source for strategy names and `winws.exe` arguments. Flowseal BAT files are treated as update input: after their parameters are imported into the catalog, the individual strategy files are removed from the installed core.

The catalog uses schema version 3:

```json
{
  "schemaVersion": 3,
  "strategies": [
    {
      "name": "general",
      "arguments": "--wf-tcp=80,443 ...",
      "source": "flowseal"
    }
  ]
}
```

Argument placeholders such as `%BIN%`, `%LISTS%`, and `%GameFilterUDP%` remain in the catalog. They are resolved immediately before launch, so a staged core can be moved into the active `binaries` directory without leaving stale absolute paths in the catalog.

Schema version 3 stores arguments exactly as `winws.exe` receives them. CMD-only caret escapes are removed during BAT import (`^!` becomes `!`), and existing version-2 catalogs are migrated atomically on first read.

## Catalog lifecycle

- A legacy installation without `strategies.json` creates it on the first strategy read or installation validation.
- A schema-version-1 catalog is rebuilt automatically while the original Flowseal BAT files are still available; a version-2 catalog is migrated without requiring the removed BAT files.
- Every downloaded core is parsed and receives a fresh catalog during validation. The imported Flowseal strategy BAT files are deleted only after the catalog has been written successfully.
- The catalog is rebuilt again after activation, once user-owned strategies have been restored.
- A custom strategy import validates the BAT content, writes it to `binaries/custom-strategies`, and rebuilds the catalog.

`service.bat` is update input, not part of the runtime. Zapret UI reads `LOCAL_VERSION` from it while validating a downloaded or legacy Flowseal package, writes that version to `.zapret-ui-core.json`, and only then removes `service.bat`. Installed-core version checks use the manifest.

## Strategy tests

The test wizard gets strategy names and fully resolved arguments from the catalog, starts `bin/winws.exe` directly, and performs HTTP, TLS, latency, and DPI probes in Rust. It does not recreate or execute strategy BAT or PowerShell files. Flowseal's `utils/test zapret.ps1` is treated as update input and removed after a valid installation manifest has been committed.

## User-owned strategies

Imported BAT files are kept in `binaries/custom-strategies`. Core activation and rollback copy that directory into the selected core before post-activation validation. If Flowseal later adds a strategy with the same name, the user's version takes precedence in the runtime catalog.

Only the parsed `winws.exe` argument suffix is used at runtime and in the test wizard. Zapret UI does not execute an imported BAT file as a script.
