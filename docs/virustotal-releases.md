# VirusTotal release checks

The `publish` workflow scans the final Windows EXE and MSI installers before publishing a new release. Both analyses must reach `completed`; a timeout, API failure, or mismatched release-asset SHA-256 fails the workflow. Detection results do not automatically block publication: release notes link to the full reports without claiming that the files are clean.

## Setup

In the GitHub repository, open **Settings → Secrets and variables → Actions → New repository secret**:

- Name: `VIRUSTOTAL_API_KEY`
- Secret: your VirusTotal API key

Do not put the key in source files, `.env`, release notes, or an Actions variable. The workflow reads it only from the secret and does not log API response bodies or request headers.

No other secret is needed: the built-in `GITHUB_TOKEN` updates the release using the existing `contents: write` permission.

## Behavior

- Selects exactly one EXE and one en-US MSI from this build, matching the application version.
- Uploads the installers through the public VirusTotal API and waits for each analysis to complete. Files over 32 MiB use the large-file upload endpoint (maximum 650 MiB).
- Spaces API calls by at least 16 seconds; checks analysis status every 30 seconds, up to 20 times per installer. HTTP 429 and server errors have bounded retries with a delay of at least 60 seconds. A normal run uses 4–44 requests before retries, depending on file size and analysis duration.
- Serializes release workflows to avoid sharing the key concurrently within this repository. Other uses of the same key still count toward its quota. The public API's noncommercial-use restrictions remain applicable.
- Verifies each completed analysis hash and GitHub release asset digest against the local installer bytes.
- Adds Russian **Безопасность** and English **Security** sections, each with the signing policy, separate EXE/MSI report links, and both SHA-256 values.
- Preserves other release notes and replaces only the section between `zapret-virustotal` HTML markers on reruns.
- Publishes a new draft only after successful scans and notes update. A manual rebuild of an already public release stays public; a failed scan does not undo files uploaded by the preceding build step. Rerun the workflow after resolving an API/quota error to refresh the reports for the rebuilt assets.

The files sent to VirusTotal are the distributable installers, not user data. Public scanning submits those files to VirusTotal; this is not its private scanning API.

Tests run without a real API key or network requests:

```sh
node --test scripts/virustotal-release.test.mjs
```

API references: [file upload](https://docs.virustotal.com/reference/files-scan), [large uploads](https://docs.virustotal.com/reference/files-upload-url), [analysis status](https://docs.virustotal.com/reference/analyses-object).
