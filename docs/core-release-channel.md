# Managed stable core channel

Core updates are controlled by `core-channel/stable.json`; the application never promotes an upstream Flowseal release automatically. The checked-in manifest is also the offline fallback embedded at compile time.

## Promotion

1. Select a Flowseal release candidate and test it manually on Windows.
2. Run `npm run promote-core-stable -- --version <version> --url <https-url> [--url <mirror>]`.
3. The script downloads every exact artifact byte stream and calculates its SHA-256. It rejects HTTP and writes the manifest atomically.
4. Review the `stable.json` diff, including every URL, version, and checksum.
5. Open a separate pull request containing the manifest promotion. The script deliberately does not commit, push, merge, or publish.
6. Once merged to `main`, clients receive the approved version without a new application release. Builds retain that manifest as their last-known-good offline fallback.

The initial placeholder manifest intentionally contains no release. The execution environment used to add the channel could not reach GitHub, so no unverifiable production version or checksum was invented. A maintainer must run the promotion command in a networked environment before releasing this build.
