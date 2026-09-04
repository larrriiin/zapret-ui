# Managed stable core channel

Core updates are controlled by `core-channel/stable.json`; the application never promotes an upstream Flowseal release automatically. Remote manifests are accepted only with a valid detached `stable.json.sig` minisign signature. The checked-in manifest is also the trusted offline fallback embedded at compile time.

## Promotion

1. Select a Flowseal release candidate and test it manually on Windows.
2. Run `npm run promote-core-stable -- --version <version> --url <https-url> [--url <mirror>]`.
3. The script downloads every exact artifact byte stream and calculates its SHA-256. It rejects HTTP and writes the manifest atomically.
4. Sign the manifest with `npm run sign-core-channel`. The command uses the same `TAURI_SIGNING_PRIVATE_KEY` identity as application updates.
5. Review both `stable.json` and `stable.json.sig`, including every URL, version, checksum, and signature change.
6. Open a separate pull request containing the manifest promotion. The script deliberately does not commit, push, merge, or publish.
7. Once merged to `main`, clients receive the approved version without a new application release. Builds retain that manifest as their last-known-good offline fallback.

The stable channel is active. Its current Flowseal version, download URL, and verified checksum are recorded in `core-channel/stable.json`.

## Automated Flowseal release check

The `Check Flowseal stable release` workflow checks the latest published, non-prerelease GitHub Release from `Flowseal/zapret-discord-youtube` every day at 06:17 UTC. It selects the release asset whose name exactly matches the normalized release version, then uses the same promotion script to download it and calculate its SHA-256. If the managed channel already contains that version, the workflow exits without creating a branch or pull request.

Maintainers can also run the check on demand from **Actions → Check Flowseal stable release → Run workflow**. The repository secret `FLOWSEAL_UPDATE_TOKEN` must contain a token that can push a branch and open a pull request; using that token ensures the resulting `pull_request` event runs the repository's normal checks. Maintainers should rotate or renew this token periodically before it expires.

For a new version, the automation changes only `core-channel/stable.json` and `core-channel/stable.json.sig`, then opens a pull request against `main`. If an existing channel has no signature, a manual workflow run creates a signature-only promotion PR. The workflow never merges the pull request, publishes ZAPRET UI, or creates a tag. A human must test the Windows installation, launch modes, strategy and user-list persistence, rollback, and restart behavior before merging the pull request.
