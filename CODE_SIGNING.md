# Code signing policy

## Current signing status

**Status: Pending — Windows installers are not yet Authenticode-signed.**

ZAPRET UI has applied to the [SignPath Foundation](https://signpath.org/) free code signing program. The initial application was not approved due to insufficient public visibility and community recognition.

As the project continues to grow, we intend to reapply for the program.

Currently, Windows installers distributed through GitHub Releases are not signed with a publicly trusted code signing certificate. Windows SmartScreen may therefore display an "Unknown publisher" warning.

The `.sig` files included in GitHub Releases are used by the Tauri updater to verify update authenticity and integrity. They are not Windows Authenticode signatures and do not replace a trusted code signing certificate.

Once the project is accepted into the SignPath Foundation program, this policy will be updated accordingly.

--- 

~~Free code signing provided by [SignPath.io](https://signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).~~

## Team roles

- Committers and reviewers: [larrriiin](https://github.com/larrriiin)
- Approvers: [larrriiin](https://github.com/larrriiin)

## Privacy policy

ZAPRET UI does not include telemetry or analytics and does not intentionally send usage statistics, Traffic Monitor data, browsing history, crash reports, or other personal user data to the project maintainers.

The application performs network requests required for its documented functionality:

- Shortly after startup, ZAPRET UI automatically checks for updates to the application and the stable Zapret core channel. These requests are currently made to GitHub-hosted resources. If direct access fails, the user may choose to retry through a fallback update proxy operated by the ZAPRET UI project maintainer or through a custom proxy supplied by the user.
- Core packages and IPSet data are downloaded when the user installs or updates the corresponding components.
- When the user explicitly starts strategy testing, ZAPRET UI performs network connectivity tests against public Internet services such as Discord, YouTube, Google, and Cloudflare. DPI testing may also download a public test-target list and send synthetic test traffic to selected test endpoints.
- When the user explicitly checks a domain in Diagnostics, ZAPRET UI connects to that domain and may also check a related service endpoint.
- The Traffic Monitor processes connection metadata, packet headers, and byte counters locally in memory. This monitoring information is not uploaded to the project maintainers.

Normal network requests inherently expose information such as the user's public IP address to the remote service being contacted, as with other Internet connections. When the maintainer-operated fallback proxy is used, it necessarily processes the user's public IP address and request-routing metadata, such as the destination host and request time; ordinary infrastructure access or security logs may retain this metadata. The proxy is used only to relay update-related requests and does not receive Traffic Monitor data, browsing history, strategy-test traffic, or application telemetry. Users can decline the fallback proxy retry and may instead retry directly or provide their own proxy.

Requests to third-party services are subject to their respective policies, including the [GitHub Privacy Statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement), [Discord Privacy Policy](https://discord.com/privacy), [Google Privacy Policy](https://policies.google.com/privacy), and [Cloudflare Privacy Policy](https://www.cloudflare.com/privacypolicy/).

## System changes

ZAPRET UI performs system-level changes only as part of documented application functions initiated or configured by the user.

Depending on the selected operating mode and settings, the application may:

- install, start, stop, or remove the `zapret` Windows service;
- use and manage the WinDivert network driver/service required by the Zapret core;
- modify configuration associated with the Zapret service;
- configure application autostart when enabled by the user;
- install, update, or roll back Zapret core files;
- modify user-managed Zapret filtering lists and settings.

Administrator privileges are required for operations that modify Windows services, drivers, or other protected system configuration.

## Removal

The application provides controls for stopping Zapret and removing its associated running service state. ZAPRET UI itself can be removed through the standard Windows installed-apps interface or its bundled uninstaller.
