# Traffic monitor classification

The monitor shows two independent attributes: **route** (direct, warp, tg_proxy, tg_proxy_warp, local, unknown) and **Zapret processing** (expected, excluded, not_matched, unknown, off). A direct route can still have expected Zapret processing. No status claims confirmed desync: the stock winws binary does not publish an acknowledgement to this monitor.

## Running rules

The exact arguments used by a successful application launch are cached. Already-running processes are read through CIM, matching the canonical path of our binaries/bin/winws.exe and requiring a single result. Reads refresh in the background every five seconds; stale responses cannot overwrite a newer launch or a stop. Unavailable arguments produce unknown, never a fallback to edited strategy settings.

Rules retain every --new profile in order. Capture ports and remote profile ports are separate. Within a profile, address family, IPSet, domains and L7 are conjunctive; exclusions reject that profile, allowing later profiles to match. The first known match wins, including a no-action profile. An unresolved earlier profile blocks a confident prediction about later profiles. Unsupported selectors, unreadable lists, autohostlist runtime state, and missing required evidence produce unknown.

Domain rules match label boundaries and honor the ^ exact-name prefix. Includes and excludes are separate per profile. Host and IPSet metadata changes invalidate the compiled rule cache. An intentionally empty IPSet retains Flowseal's any semantics.

## Observed evidence

TCP evidence reads HTTP/1 Host and cleartext TLS ClientHello SNI, including contiguous TCP fragments, overlaps and multiple TLS records. ECH outer names, gaps, duplicate Host fields, malformed records and oversized handshakes are not treated as destination evidence. UDP STUN/QUIC header recognition supplies protocol evidence; QUIC hostnames and unsupported L7 protocols remain unknown. No reverse-DNS or shared-IP hostname guesses are made.

Only the first 512 tracked flows can allocate a transient handshake buffer, capped at 16 KiB per flow with a three-second assembly window. Buffers are released on completion/failure or flow expiry. Connection details are capped at 4096 flows; raw counters continue if the cap is reached and the UI reports incomplete classification. No packet contents are written to disk or sent elsewhere. The monitor does not decrypt HTTPS.

## Route evidence

WARP application and site relays expose exact original/local/upstream endpoint pairs after a successful SOCKS handshake. Closed mappings are retained briefly for attribution, with close timestamps preventing reuse by a new session on the same ports. Packet interface indexes are resolved with GetIfEntry2: Cloudflare WARP interfaces identify a tunnel; observed Ethernet/Wi-Fi identify a direct interface, and unfamiliar interfaces remain unknown. Fixed private addresses and destination IPs alone never establish WARP routing.

Telegram attribution uses the PID of our live proxy child and its configured listener. This is evaluated before generic local-address classification. Proxy routing and a WARP interface/mapping can combine into tg_proxy_warp. Proxy connections retain unknown Zapret processing because the outer connection may be handled separately.

## Counters and history

The connections table groups visible flows by case-insensitive process name and remote IP, including different PIDs, ports and protocols. Rates and byte totals are summed after filtering. Each row shows its connection count; tooltips retain PIDs, ports, hostnames and classification reasons. Different routes and Zapret states remain separate badges within the row. The summary counts underlying connections, and the journal retains individual events.

Estimated Zapret bytes describe matching connections, not bytes actually modified by desync. Initial unknown bytes are attributed when handshake evidence becomes available; later exclusions or proxy evidence remove estimates without changing raw traffic totals. Rule changes reset estimates using per-flow byte baselines. Reused TCP tuples discard prior hostname evidence, while archived session totals remain intact. The journal records classification changes as well as network events.

Run cargo test --lib and npm run test:traffic. Live capture and real WARP/TG tunnel integration additionally require a Windows session with the relevant modules running.
