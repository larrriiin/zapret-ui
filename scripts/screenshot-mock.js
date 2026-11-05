// Mock Tauri IPC surface and demo state for automated screenshot generation.
// This file runs exclusively during screenshot generation and does not affect production.
import appPackage from '../package.json';
import stableCore from '../core-channel/stable.json';
import telegramModule from '../src-tauri/telegram-module.json';

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
  /* The capture viewport cannot scroll. Absolute positioning avoids Chromium
     dropping the transparent fixed header when capturing a clipped body. */
  .app-header { position: absolute !important; }
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
  start_with_zapret: true,
};

const mockTrafficSnapshot = {
  running: true,
  filter_active: true,
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
      hostname: 'discord.com',
      state: 'established',
      route: 'warp',
      zapret: { state: 'unknown', reason: 'proxy' },
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
      hostname: 'youtube.com',
      state: 'established',
      route: 'direct',
      zapret: { state: 'expected', reason: 'hostlist', profile: 1 },
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
      hostname: 'telegram.org',
      state: 'established',
      route: 'tg_proxy',
      zapret: { state: 'unknown', reason: 'proxy' },
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
      hostname: 'steamcommunity.com',
      state: 'established',
      route: 'direct',
      zapret: { state: 'excluded', reason: 'excluded' },
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
  { name: 'System Proxy', passed: true, message: 'No system proxy detected' },
  { name: 'TCP Timestamps', passed: true, message: 'TCP timestamps are enabled' },
  { name: 'Adguard', passed: true, message: 'Adguard not detected' },
  { name: 'Killer Network Service', passed: true, message: 'Killer services not detected' },
  { name: 'Intel Connectivity Network Service', passed: true, message: 'Intel Connectivity service not detected' },
  { name: 'Check Point', passed: true, message: 'Check Point services not detected' },
  { name: 'SmartByte', passed: true, message: 'SmartByte services not detected' },
  { name: 'VPN Services', passed: true, message: 'No VPN services detected' },
  { name: 'Secure DNS', passed: true, message: 'Secure DNS is configured' },
  { name: 'Hosts File', passed: true, message: 'No YouTube entries in hosts file' },
  { name: 'WinDivert', passed: true, message: 'WinDivert driver is running' },
];

const mockRules = [
  { id: 'game', name: 'Игры', icon: 'sports_esports', path: 'C:\\Games\\Game\\game.exe', enabled: true, zapret: 'keep', strategy: null, warp: 'on', warp_mode: 'proxy', warp_routes: null, restore: true, close_off: false },
  { id: 'discord', name: 'Discord', icon: 'forum', path: 'C:\\Apps\\Discord\\Discord.exe', enabled: true, zapret: 'on', strategy: 'general (ALT12)', warp: 'keep', warp_mode: null, warp_routes: null, restore: true, close_off: false },
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
          return { game_filter: 'disabled', ipset: 'any', game_tcp_range: '1024-65535', game_udp_range: '1024-65535' };
        case 'get_fakes_info':
          return {
            current_discord_fake: 'quic_initial_steamcommunity_com',
            current_game_fake: 'quic_initial_dbankcloud_ru',
            available_fakes: [
              { name: 'quic_initial_steamcommunity_com', filename: 'quic_initial_steamcommunity_com.bin' },
              { name: 'quic_initial_dbankcloud_ru', filename: 'quic_initial_dbankcloud_ru.bin' },
              { name: 'quic_initial_www_google_com', filename: 'quic_initial_www_google_com.bin' },
            ],
          };
        case 'get_ui_version_cmd':
          return appPackage.version;
        case 'get_local_version_cmd':
        case 'get_core_version_cmd':
          return stableCore.version;
        case 'get_core_update_info':
        case 'check_core_update':
          return { status: 'up_to_date', currentVersion: stableCore.version, stableVersion: stableCore.version };
        case 'get_core_installation_state':
          return {
            currentVersion: stableCore.version,
            previousVersion: '1.10.3',
            rollbackAvailable: true,
          };
        case 'get_warp_status':
          return {
            installed: true,
            connected: true,
            state: 'connected',
            mode: 'proxy',
            modes: ['doh', 'dot', 'warp', 'warp+dot', 'warp+doh', 'proxy', 'tunnel_only'],
            version: '2026.7.1343.0',
            protocol: 'MASQUE', protocol_editable: true, diag_supported: true,
            proxy: { address: '127.0.0.1', port: 40000, kind: 'SOCKS5', active: true, port_editable: true },
            sites: { domains: ['discord.com', 'youtube.com', 'googlevideo.com'], enabled: true, runtime_active: true, connections: [] },
            applications: { paths: ['C:\\Apps\\Discord\\Discord.exe', 'C:\\Games\\Game\\game.exe'], enabled: true, runtime_active: true, active_connections: 2, total_connections: 8, failed_connections: 0, connections: [] },
            automation: { restore_rules: true }, last_errors: [], connections: [], error: null,
          };
        case 'get_application_rules':
          return { config: { enabled: true, rules: structuredClone(mockRules) }, active: ['game'], error: null, paused: false };
        case 'get_telegram_status':
          return {
            installed: true,
            running: true,
            busy: false,
            version: telegramModule.version,
            download_bytes: 16923850,
            port: 1443,
            host: '127.0.0.1',
          };
        case 'get_telegram_config':
          return structuredClone(mockConfig);
        case 'read_user_list':
          if (args?.filename === 'list-general-user.txt') {
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
          if (args?.filename === 'list-exclude-user.txt') {
            return ['gosuslugi.ru', 'mos.ru', 'sberbank.ru', 'tbank.ru', 'yandex.ru', 'vk.com'];
          }
          if (args?.filename === 'list-ip.txt') {
            return ['162.158.0.0/15', '104.16.0.0/12', '149.154.167.220', '91.108.56.0/22'];
          }
          if (args?.filename === 'ipset-exclude-user.txt') {
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
          return { checks: mockDiagnostics.map(({ passed, ...check }) => ({ ...check, status: passed ? 'passed' : 'warning', link: null })), vpn_services: null };
        case 'get_setup_state':
        case 'precheck_tests':
          return { strategies_count: 5, is_admin: true };
        case 'ensure_binaries_present':
          return true;
        case 'telegram_logs':
          return `INFO: TG WS Proxy ${telegramModule.version} active on 127.0.0.1:1443\nINFO: Connection handled`;
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
      onResized: async () => () => {},
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
