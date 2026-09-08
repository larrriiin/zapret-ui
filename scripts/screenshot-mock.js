// Mock Tauri IPC surface and demo state for automated screenshot generation.
// This file runs exclusively during screenshot generation and does not affect production.

localStorage.setItem('zapret_lang', 'ru');
localStorage.setItem('zapret_theme', 'zapret');

// Inject screenshot styling: transparent root, rounded window corners, disabled animations
const style = document.createElement('style');
style.id = 'screenshot-window-style';
style.textContent = `
  html, body {
    background: transparent !important;
    background-color: transparent !important;
    margin: 0 !important;
    padding: 0 !important;
    overflow: hidden !important;
    width: 1100px !important;
    height: 980px !important;
  }
  body {
    width: 1100px !important;
    height: 980px !important;
    margin: 0 !important;
    padding: 0 !important;
    box-sizing: border-box !important;
    clip-path: inset(0 round 12px) !important;
    transform: translate(0) !important;
    overflow: hidden !important;
  }
  body::before {
    content: '' !important;
    position: fixed !important;
    inset: 0 !important;
    background: var(--color-background, #070d1f) !important;
    z-index: -9999 !important;
    pointer-events: none !important;
  }
  body::after {
    content: '' !important;
    position: fixed !important;
    inset: 0 !important;
    border: 1px solid rgba(255, 255, 255, 0.12) !important;
    border-radius: 12px !important;
    pointer-events: none !important;
    z-index: 99999 !important;
  }
  :root[data-setup-window] body {
    background: transparent !important;
  }
  :root[data-setup-window] body::before {
    background: var(--color-popup, #0d1225) !important;
  }
  :root[data-setup-window] .setup-dialog::backdrop {
    background: transparent !important;
  }
  :root[data-setup-window] .setup-dialog {
    border-radius: 12px !important;
    overflow: hidden !important;
    clip-path: inset(0 round 12px) !important;
  }
  *, *::before, *::after {
    animation-duration: 0s !important;
    animation-delay: 0s !important;
    transition-duration: 0s !important;
    transition-delay: 0s !important;
  }
  .status-lamp.is-on {
    animation: none !important;
    opacity: 1 !important;
    background: linear-gradient(90deg, transparent 0%, var(--color-primary) 30%, var(--color-primary-dim) 70%, transparent 100%) !important;
    box-shadow: 0 0 18px color-mix(in srgb, var(--color-primary) 65%, transparent), 0 0 28px color-mix(in srgb, var(--color-primary-dim) 35%, transparent) !important;
  }
`;
document.head.appendChild(style);

const eventHandlers = {};

const mockConfig = {
  host: '127.0.0.1',
  port: 1443,
  secret: 'e3b0c44298fc1c149afbf4c8996fb924',
  dc_ips: { '2': '149.154.167.220', '4': '149.154.167.220' },
  cfproxy: false,
  cfproxy_domains: [],
  worker: false,
  worker_domains: [],
};

const mockTrafficSnapshot = {
  running: true,
  error: null,
  upload_bps: 1420000,
  download_bps: 12850000,
  upload_bytes: 58200000,
  download_bytes: 428000000,
  zapret_upload_bps: 1250000,
  zapret_download_bps: 11400000,
  zapret_upload_bytes: 49000000,
  zapret_download_bytes: 385000000,
  connections: [
    {
      pid: 1284,
      process_name: 'Discord.exe',
      local_address: '192.168.1.105',
      local_port: 54122,
      remote_address: '162.159.130.233',
      remote_port: 443,
      protocol: 'TCP',
      system_process: false,
      zapret_candidate: true,
      upload_bps: 45000,
      download_bps: 890000,
      upload_bytes: 3200000,
      download_bytes: 42000000,
      zapret_upload_bps: 45000,
      zapret_download_bps: 890000,
      zapret_upload_bytes: 3200000,
      zapret_download_bytes: 42000000,
    },
    {
      pid: 4592,
      process_name: 'chrome.exe',
      local_address: '192.168.1.105',
      local_port: 58490,
      remote_address: '142.250.74.206',
      remote_port: 443,
      protocol: 'TCP',
      system_process: false,
      zapret_candidate: true,
      upload_bps: 120000,
      download_bps: 10200000,
      upload_bytes: 12000000,
      download_bytes: 320000000,
      zapret_upload_bps: 120000,
      zapret_download_bps: 10200000,
      zapret_upload_bytes: 12000000,
      zapret_download_bytes: 320000000,
    },
    {
      pid: 8912,
      process_name: 'Telegram.exe',
      local_address: '127.0.0.1',
      local_port: 1443,
      remote_address: '149.154.167.220',
      remote_port: 443,
      protocol: 'TCP',
      system_process: false,
      zapret_candidate: true,
      upload_bps: 24000,
      download_bps: 185000,
      upload_bytes: 1400000,
      download_bytes: 14500000,
      zapret_upload_bps: 24000,
      zapret_download_bps: 185000,
      zapret_upload_bytes: 1400000,
      zapret_download_bytes: 14500000,
    },
    {
      pid: 6120,
      process_name: 'steam.exe',
      local_address: '192.168.1.105',
      local_port: 51230,
      remote_address: '23.48.201.88',
      remote_port: 443,
      protocol: 'TCP',
      system_process: false,
      zapret_candidate: false,
      upload_bps: 5000,
      download_bps: 12000,
      upload_bytes: 520000,
      download_bytes: 1800000,
      zapret_upload_bps: 0,
      zapret_download_bps: 0,
      zapret_upload_bytes: 0,
      zapret_download_bytes: 0,
    },
  ],
  events: [
    {
      timestamp_ms: Date.now() - 3000,
      process_name: 'Discord.exe',
      remote_address: '162.159.130.233',
      remote_port: 443,
      protocol: 'TCP',
      event_type: 'connected',
    },
    {
      timestamp_ms: Date.now() - 1000,
      process_name: 'chrome.exe',
      remote_address: '142.250.74.206',
      remote_port: 443,
      protocol: 'TCP',
      event_type: 'observed',
    },
  ],
};

const mockDiagnostics = [
  { name: 'Base Filtering Engine', passed: true, message: 'Service is running' },
  { name: 'System Proxy', passed: true, message: 'Proxy check passed' },
  { name: 'TCP Timestamps', passed: true, message: 'TCP timestamps are enabled' },
  { name: 'Adguard', passed: true, message: 'Adguard check passed' },
  { name: 'Killer Network Service', passed: true, message: 'Killer check passed' },
  { name: 'Intel Connectivity Network Service', passed: true, message: 'Intel Connectivity check passed' },
  { name: 'Check Point', passed: true, message: 'Check Point check passed' },
  { name: 'SmartByte', passed: true, message: 'SmartByte check passed' },
  { name: 'VPN Services', passed: true, message: 'VPN check passed' },
  { name: 'Secure DNS', passed: true, message: 'Secure DNS is configured' },
  { name: 'Hosts File', passed: true, message: 'No YouTube entries in hosts file' },
  { name: 'WinDivert', passed: true, message: 'WinDivert driver is running' },
];

window.__TAURI__ = {
  core: {
    invoke: async (cmd, args) => {
      switch (cmd) {
        case 'check_admin_privileges':
          return true;
        case 'show_app_window':
          return null;
        case 'get_strategies':
          return ['general (ALT12)', 'general (ALT)', 'general', 'discord', 'youtube', 'mgo (ALT)', 'rkn'];
        case 'get_zapret_status':
          return { running: true, strategy: 'general (ALT12)', mode: 'service', active: true };
        case 'check_status_full':
          return {
            running: true,
            strategy: 'general (ALT12)',
            windivert_service: 'running',
            bypass_process: 'running',
            zapret_service: 'running',
            mode: 'service',
          };
        case 'get_filters_status':
          return { game_filter: 'disabled', ipset_filter: 'any' };
        case 'get_fakes_info':
          return {
            active_tls: 'google',
            active_http: 'default',
            active_udp: 'default',
            available_fakes: [
              { name: 'google', type: 'tls' },
              { name: 'default', type: 'http' },
              { name: 'cloudflare', type: 'tls' },
            ],
          };
        case 'get_ui_version_cmd':
          return '26.9.13';
        case 'get_local_version_cmd':
        case 'get_core_version_cmd':
          return '1.8.6';
        case 'get_core_update_info':
        case 'check_core_update':
          return { status: 'up_to_date', currentVersion: '1.8.6', stableVersion: '1.8.6' };
        case 'get_core_installation_state':
          return {
            currentVersion: '1.8.6',
            previousVersion: '1.8.5',
            rollbackAvailable: true,
          };
        case 'get_warp_status':
          return {
            installed: true,
            connected: true,
            state: 'connected',
            mode: 'warp',
            modes: ['warp', 'doh', 'dot', 'proxy'],
            version: '2026.7.1343.0',
            proxy: null,
          };
        case 'get_telegram_status':
          return {
            installed: true,
            running: true,
            busy: false,
            version: '1.10.2',
            download_bytes: 16923850,
            port: 1443,
            host: '127.0.0.1',
          };
        case 'get_telegram_config':
          return structuredClone(mockConfig);
        case 'read_user_list':
          if (args?.filename === 'list-general.txt') {
            return [
              'discord.com',
              'discordapp.com',
              'discord.gg',
              'youtube.com',
              'googlevideo.com',
              'ytimg.com',
              'instagram.com',
              'twitter.com',
              'x.com',
              'notion.so',
              'medium.com',
            ];
          }
          if (args?.filename === 'list-exclude.txt') {
            return ['gosuslugi.ru', 'mos.ru', 'sberbank.ru', 'tbank.ru', 'yandex.ru', 'vk.com'];
          }
          if (args?.filename === 'list-ip.txt') {
            return ['162.158.0.0/15', '104.16.0.0/12', '149.154.167.220', '91.108.56.0/22'];
          }
          if (args?.filename === 'list-ip-exclude.txt') {
            return ['192.168.0.0/16', '10.0.0.0/8', '127.0.0.1'];
          }
          return [];
        case 'get_traffic_snapshot':
          return structuredClone(mockTrafficSnapshot);
        case 'start_traffic_monitor':
          return structuredClone(mockTrafficSnapshot);
        case 'stop_traffic_monitor':
          return { ...mockTrafficSnapshot, running: false };
        case 'run_diagnostics':
          return structuredClone(mockDiagnostics);
        case 'get_setup_state':
        case 'precheck_tests':
          return { strategies_count: 5, is_admin: true };
        case 'ensure_binaries_present':
          return true;
        case 'telegram_logs':
          return 'INFO: TG WS Bridge Proxy 1.10.2 active on 127.0.0.1:1443\\nINFO: Secret: [secret]\\nINFO: Connection handled';
        default:
          return null;
      }
    },
  },
  event: {
    listen: async (name, handler) => {
      eventHandlers[name] = handler;
      return () => delete eventHandlers[name];
    },
    emit: async (name, payload) => {
      if (eventHandlers[name]) eventHandlers[name]({ payload });
    },
  },
  window: {
    getCurrentWindow: () => ({
      minimize: async () => {},
      maximize: async () => {},
      close: async () => {},
      isMaximized: async () => false,
      startDragging: async () => {},
    }),
  },
  updater: {
    check: async () => null,
  },
  process: {
    relaunch: async () => {},
    exit: async () => {},
  },
  opener: {
    openUrl: async () => {},
  },
};
