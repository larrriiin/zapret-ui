// ─── i18n Engine ─────────────────────────────────────────────────────────────

let currentLang = 'ru';

function initI18n() {
    const saved = localStorage.getItem('zapret_lang');
    if (saved) {
        currentLang = saved;
    } else {
        const sysLang = navigator.language || navigator.userLanguage;
        currentLang = sysLang.startsWith('ru') ? 'ru' : 'en';
    }
    updatePageTranslations();
}

function t(key, params = {}) {
    const dict = currentLang === 'ru' ? window.i18n_ru : window.i18n_en;
    let translation = dict[key] || key;
    
    // Simple placeholder replacement: {name} -> params.name
    Object.keys(params).forEach(p => {
        translation = translation.replace(`{${p}}`, params[p]);
    });
    
    return translation;
}

function updatePageTranslations() {
    document.querySelectorAll('[data-i18n]').forEach(el => {
        const key = el.dataset.i18n;
        const attr = el.dataset.i18nAttr;
        const translation = t(key);
        
        if (attr) {
            el.setAttribute(attr, translation);
        } else {
            // Check if it's an input with placeholder
            if (el.tagName === 'INPUT' && el.placeholder) {
                el.placeholder = translation;
            } else {
                el.textContent = translation;
            }
        }
    });
    
    // Update language switcher text
    const langBtn = $('lang-text');
    if (langBtn) langBtn.textContent = currentLang.toUpperCase();
}

function toggleLanguage() {
    currentLang = currentLang === 'ru' ? 'en' : 'ru';
    localStorage.setItem('zapret_lang', currentLang);
    updatePageTranslations();
    
    // Force some UI updates
    pollStatus();
    pollFilters();
    loadStrategies();
    
    // Update specific dynamic areas
    if (typeof updateDiagnosticsUI === 'function') updateDiagnosticsUI();

    // Refresh open info modal if any
    if (typeof refreshOpenInfoModal === 'function') refreshOpenInfoModal();

    // Sync language with tray
    syncTrayLocalization();
}

async function syncTrayLocalization() {
    try {
        await invoke('update_tray_translations', {
            translations: {
                exit: t('tray_exit'),
                show: t('tray_show'),
                status_prefix: t('tray_status_prefix'),
                strategy_prefix: t('tray_strategy_prefix'),
                toggle_on: t('tray_toggle_on'),
                toggle_off: t('tray_toggle_off'),
                change_strategy: t('tray_change_strategy'),
                minimized_title: t('tray_minimized_title'),
                minimized_body: t('tray_minimized_body'),
                status_on: t('connected'),
                status_off: t('disconnected')
            }
        });
    } catch (err) {
        console.error('Failed to sync tray localization:', err);
    }
}

// invoke получаем лениво, чтобы не было гонки с инициализацией Tauri
function invoke(cmd, args) {
    return window.__TAURI__.core.invoke(cmd, args);
}

const listen = (event, handler) => {
    return window.__TAURI__.event.listen(event, handler);
}

const $ = id => document.getElementById(id);


// ─── Custom Strategy Dropdown ─────────────────────────────────────────────────

let _strategyValue = '';

function setStrategyValue(value, label) {
    _strategyValue = value;
    const lbl = $('strategy-label');
    if (lbl) {
        lbl.textContent = label;
        lbl.classList.remove('text-on-surface/60');
        lbl.classList.add('text-on-surface');
    }
    const sel = $('strategy-select');
    if (sel) sel.value = value;
}

function getStrategyValue() {
    return _strategyValue;
}

function closeStrategyDropdown() {
    const panel = $('strategy-options');
    const chevron = $('strategy-chevron');
    if (panel) panel.classList.add('hidden');
    if (chevron) chevron.style.transform = '';
}

function initStrategyDropdown() {
    const trigger = $('strategy-trigger');
    const panel   = $('strategy-options');
    const chevron = $('strategy-chevron');
    if (!trigger || !panel) return;

    // Use fixed positioning so overflow-hidden parents don't clip the panel
    trigger.addEventListener('click', (e) => {
        e.stopPropagation();
        const isOpen = !panel.classList.contains('hidden');
        if (isOpen) {
            closeStrategyDropdown();
        } else {
            const rect = trigger.getBoundingClientRect();
            panel.style.top    = (rect.bottom + 4) + 'px';
            panel.style.left   = rect.left + 'px';
            panel.style.width  = rect.width + 'px';
            panel.classList.remove('hidden');
            chevron.style.transform = 'rotate(180deg)';
        }
    });
    document.addEventListener('click', (e) => {
        if (!$('strategy-dropdown')?.contains(e.target)) closeStrategyDropdown();
    });
}

// ─── Стратегии ────────────────────────────────────────────────────────────────

async function loadStrategies() {
    const list = $('strategy-options-list');
    const sel  = $('strategy-select');
    try {
        const strategies = await invoke('get_strategies');
        if (list) list.innerHTML = '';
        if (sel)  sel.innerHTML  = '';

        if (!strategies || strategies.length === 0) {
            $('strategy-label').textContent = t('no_strategies');
            return;
        }

        strategies.forEach(name => {
            const opt = document.createElement('option');
            opt.value = name;
            opt.textContent = name;
            if (sel) sel.appendChild(opt);

            if (list) {
                const item = document.createElement('button');
                item.type = 'button';
                item.dataset.value = name;
                item.className = 'w-full text-left px-4 py-2.5 text-sm font-headline text-on-surface hover:bg-primary/10 transition-colors flex items-center gap-2';
                item.innerHTML = '<span class="material-symbols-outlined text-sm text-primary/30 item-icon">chevron_right</span><span>' + name + '</span>';
                item.addEventListener('click', () => {
                    list.querySelectorAll('button').forEach(b => {
                        b.classList.remove('bg-primary/20', 'text-primary');
                        b.querySelector('.item-icon').style.opacity = '0.3';
                    });
                    item.classList.add('bg-primary/20', 'text-primary');
                    item.querySelector('.item-icon').style.opacity = '1';
                    setStrategyValue(name, name);
                    closeStrategyDropdown();

                    // Если сервис запущен, сразу переключаем на новую стратегию
                    invoke('get_zapret_status').then(status => {
                        if (status.running) {
                            showRestartStatus(t('switching_strategy'), true);
                            invoke('start_zapret', { strategy: name, mode: status.mode || 'service' }).then(() => {
                                setTimeout(pollStatus, 500); // Даем время на старт и обновляем UI
                            });
                        }
                    });
                });
                list.appendChild(item);
            }
        });

        const defaultName = strategies.includes('general') ? 'general' : strategies[0];
        setStrategyValue(defaultName, defaultName);
        if (list) {
            list.querySelectorAll('button').forEach(b => {
                if (b.dataset.value === defaultName) {
                    b.classList.add('bg-primary/20', 'text-primary');
                    b.querySelector('.item-icon').style.opacity = '1';
                }
            });
        }

    } catch (err) {
        console.error('Error loading strategies:', err);
        $('strategy-label').textContent = t('error') + ': ' + err;
    }
}

// ─── Статус zapret ────────────────────────────────────────────────────────────

function updateStatusUI(status) {
    const trigger = $('strategy-trigger');
    const tempBtn = $('connect-temp-btn');

    if (status.running) {
        const label = status.strategy ?? t('status_connected');

        $('header-status').innerHTML = `<span class="text-primary"><span data-i18n="status_label">${t('status_label')}</span>:</span> <span class="text-secondary">${label}</span>`;
        
        $('hero-status').textContent = t('status_connected');
        $('hero-status').className = 'text-secondary';

        $('connect-btn-text').textContent = t('stop_service');
        $('connect-btn-icon').textContent = 'power_settings_new';
        $('connect-btn').dataset.action = 'stop';

        if (tempBtn) {
            tempBtn.disabled = true;
            tempBtn.classList.add('hidden');
        }

        // Update custom dropdown label to active strategy
        if (status.strategy) {
            setStrategyValue(status.strategy, status.strategy);
        }
        // if (trigger) trigger.disabled = true; // Разрешаем смену стратегии во время работы

    } else {
        $('header-status').innerHTML = `<span class="text-primary"><span data-i18n="status_label">${t('status_label')}</span>:</span> <span class="text-error-dim" data-i18n="status_disconnected">${t('status_disconnected')}</span>`;

        $('hero-status').textContent = t('status_disconnected');
        $('hero-status').className = 'text-error-dim';

        $('connect-btn-text').textContent = t('run_service');
        $('connect-btn-icon').textContent = 'bolt';
        $('connect-btn').dataset.action = 'start';

        if (tempBtn) {
            tempBtn.disabled = false;
            tempBtn.classList.remove('hidden');
        }

        if (trigger) trigger.disabled = false;
    }
}

async function pollStatus() {
    try {
        const status = await invoke('get_zapret_status');
        updateStatusUI(status);
    } catch (err) {
        console.error('Ошибка опроса статуса:', err);
    }
}

// ─── Фильтры ─────────────────────────────────────────────────────────────────

function setCardActive(id, active) {
    const el = $(id);
    if (!el) return;
    if (active) {
        el.classList.add('card-active');
    } else {
        el.classList.remove('card-active');
    }
}

function setToggle(id, on) {
    const btn = $(id);
    if (!btn) return;
    if (on) {
        btn.classList.remove('is-off');
        btn.classList.add('is-on');
    } else {
        btn.classList.remove('is-on');
        btn.classList.add('is-off');
    }
}

function setCardDisabled(id, disabled) {
    const el = $(id);
    if (!el) return;
    if (disabled) {
        el.classList.add('card-disabled');
        el.style.pointerEvents = 'none';
        el.style.opacity = '0.4';
    } else {
        el.classList.remove('card-disabled');
        el.style.pointerEvents = 'auto';
        el.style.opacity = '1';
    }
}

// Хранение предыдущих состояний для восстановления при включении
let previousGameFilter = 'all';
let previousIPSet = 'any';

function updateFiltersUI(filters) {
    console.log('updateFiltersUI called with:', JSON.stringify(filters));
    // ── IPSet ──
    // 'none' - выключен, 'any'/'loaded' - включен
    const ipsetOn = filters.ipset !== 'none';
    console.log('IPSet on:', ipsetOn, 'state:', filters.ipset, 'condition:', filters.ipset, '!== "none" =', filters.ipset !== 'none');
    setToggle('ipset-toggle', ipsetOn);
    console.log('Setting ipset-loaded active:', filters.ipset === 'loaded');
    console.log('Setting ipset-any active:', filters.ipset === 'any');
    setCardActive('ipset-loaded', filters.ipset === 'loaded');
    setCardActive('ipset-any', filters.ipset === 'any');
    // Деактивируем кнопки режимов когда выключено (none)
    setCardDisabled('ipset-loaded', !ipsetOn);
    setCardDisabled('ipset-any', !ipsetOn);

    // ── Game Filter ──
    // 'disabled' - выключен, 'all'/'tcp'/'udp' - включен
    const gameOn = filters.game_filter !== 'disabled';
    console.log('Game Filter on:', gameOn, 'state:', filters.game_filter);
    setToggle('game-toggle', gameOn);
    setCardActive('game-all', filters.game_filter === 'all');
    setCardActive('game-tcp', filters.game_filter === 'tcp');
    setCardActive('game-udp', filters.game_filter === 'udp');
    // Деактивируем кнопки режимов когда выключено
    setCardDisabled('game-all', !gameOn);
    setCardDisabled('game-tcp', !gameOn);
    setCardDisabled('game-udp', !gameOn);
}

async function pollFilters() {
    try {
        const filters = await invoke('get_filters_status');
        updateFiltersUI(filters);
    } catch (err) {
        console.error('Ошибка опроса фильтров:', err);
    }
}

// Функция для отображения статуса перезапуска
function showRestartStatus(message, isRestarting = false) {
    const el = $('hero-status');
    el.textContent = message;
    
    // Добавляем анимацию
    el.classList.remove('animate-status-change');
    void el.offsetWidth; // Триггер пересчета стилей для перезапуска анимации
    el.classList.add('animate-status-change');

    if (isRestarting) {
        el.className = 'text-primary animate-status-change';
        $('header-status').innerHTML = `<span class="text-primary"><span data-i18n="status_label">${t('status_label')}</span>:</span> <span class="text-primary" data-i18n="status_restarting">${t('status_restarting')}</span>`;
    } else {
        el.className = 'text-secondary animate-status-change';
    }
}

// Функция для перезапуска сервиса если он запущен
async function restartServiceIfRunning() {
    const status = await invoke('get_zapret_status');
    if (status.running && status.strategy) {
        showRestartStatus(t('restarting'), true);
        try {
            await invoke('stop_zapret');
            // Небольшая задержка для корректной остановки
            await new Promise(r => setTimeout(r, 1000));
            await invoke('start_zapret', { strategy: status.strategy, mode: status.mode || 'service' });
            showRestartStatus(t('status_connected'));
            await pollStatus();
            setTimeout(() => pollStatus(), 2000);
        } catch (err) {
            console.error('Ошибка перезапуска:', err);
            showRestartStatus(t('restart_failed') + ': ' + err);
        }
    }
}

async function handleGameFilterChange(mode) {
    console.log('handleGameFilterChange called with mode:', mode);
    try {
        await invoke('set_game_filter', { mode });
        await pollFilters();
        // Перезапускаем сервис если запущен
        await restartServiceIfRunning();
    } catch (err) {
        console.error('Ошибка смены Game Filter:', err);
    }
}

async function handleIPSetFilterChange(mode) {
    console.log('handleIPSetFilterChange called with mode:', mode);
    try {
        await invoke('set_ipset_filter', { mode });
        console.log('set_ipset_filter succeeded');
        await pollFilters();
        // Перезапускаем сервис если запущен
        await restartServiceIfRunning();
    } catch (err) {
        console.error('Ошибка смены IPSet Filter:', err);
    }
}

// ─── Кнопка Connect / Disconnect ─────────────────────────────────────────────

async function handleConnectClick(event) {
    // Определяем по какой кнопке кликнули
    const btn = event.currentTarget;
    const action = btn.dataset.action;
    const mode = btn.dataset.mode || 'service';

    const mainBtn = $('connect-btn');
    const tempBtn = $('connect-temp-btn');

    if (mainBtn) mainBtn.disabled = true;
    if (tempBtn) tempBtn.disabled = true;

    try {
        if (action === 'start') {
            const strategy = getStrategyValue();
            if (!strategy) return;
            $('hero-status').textContent = mode === 'service' ? t('starting_service') : t('starting_temp');
            $('hero-status').className = 'text-secondary';
            await invoke('start_zapret', { strategy, mode });
            $('hero-status').textContent = t('service_started');
        } else {
            $('hero-status').textContent = t('stopping');
            await invoke('stop_zapret');
            $('hero-status').textContent = t('disconnected');
        }
        await pollStatus();
    } catch (err) {
        console.error('Ошибка действия:', err);
        $('hero-status').textContent = `${t('error')}: ${err}`;
        $('hero-status').className = 'text-error-dim text-2xl';
        setTimeout(pollStatus, 3000);
    } finally {
        if (mainBtn) mainBtn.disabled = false;
        if (tempBtn) tempBtn.disabled = false;
        await pollStatus(); // Обновит UI (кнопки и т.д.)
    }
}

// ─── Инициализация ────────────────────────────────────────────────────────────

// Текущее состояние фильтров (глобальное для доступа из обработчиков)
let currentFilters = { game_filter: 'disabled', ipset: 'any' };

// Переопределяем pollFilters чтобы сохранять состояние
const originalPollFilters = pollFilters;
pollFilters = async function() {
    try {
        const filters = await invoke('get_filters_status');
        console.log('pollFilters received:', filters);
        currentFilters = filters;
        // Обновляем предыдущие состояния если фильтр включен
        if (filters.game_filter !== 'disabled') {
            previousGameFilter = filters.game_filter;
        }
        if (filters.ipset !== 'none') {
            previousIPSet = filters.ipset;
        }
        updateFiltersUI(filters);
    } catch (err) {
        console.error('Ошибка опроса фильтров:', err);
    }
};

// ─── Navigation ───────────────────────────────────────────────────────────────

function showSection(sectionId) {
    // Navigation Guard Logic - Trigger on ANY section change if pending
    if (pendingRestart && !restartGuardDismissed && sectionId !== currentSectionId) {
        window.pendingNavId = sectionId; 
        showRestartModal();
        return;
    }
    
    // Safety check: if we are already in this section, do nothing
    if (sectionId === currentSectionId) return;

    currentSectionId = sectionId;

    // Hide all sections
    $('section-home').classList.add('hidden');
    $('section-sites').classList.add('hidden');
    $('section-ips').classList.add('hidden');
    $('section-diagnostics').classList.add('hidden');
    
    // Show selected section
    const section = $(`section-${sectionId}`);
    if (section) {
        section.classList.remove('hidden');
    }
    
    // Update nav active states
    document.querySelectorAll('aside nav a').forEach(a => {
        a.classList.remove('border-r-2', 'border-[#ba9eff]', 'bg-gradient-to-r', 'from-[#ba9eff]/10', 'to-transparent', 'text-[#ba9eff]');
        a.classList.add('text-[#dfe4fe]/40');
    });
    
    const activeNav = sectionId === 'home' ? document.querySelector('aside nav a:first-child') : $(`nav-${sectionId}`);
    if (activeNav) {
        activeNav.classList.remove('text-[#dfe4fe]/40');
        activeNav.classList.add('border-r-2', 'border-[#ba9eff]', 'bg-gradient-to-r', 'from-[#ba9eff]/10', 'to-transparent', 'text-[#ba9eff]');
    }
}

// ─── User Lists Management ────────────────────────────────────────────────────

let pendingRestart = false;
let restartGuardDismissed = false;
let currentSectionId = 'home';

function cleanAndValidateDomain(domain) {
    let cleaned = domain.trim().toLowerCase();
    // Strip protocols
    cleaned = cleaned.replace(/^https?:\/\//, '');
    // Strip www.
    cleaned = cleaned.replace(/^www\./, '');
    // Strip trailing slashes or paths
    cleaned = cleaned.split('/')[0];
    
    // Regex for valid domain
    const domainRegex = /^([a-z0-9]+(-[a-z0-9]+)*\.)+[a-z]{2,}$/;
    return domainRegex.test(cleaned) ? cleaned : null;
}

function validateIP(ip) {
    const cleaned = ip.trim();
    // IPv4 with optional CIDR
    const ipRegex = /^(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)(?:\/(?:3[0-2]|[12]?[0-9]))?$/;
    return ipRegex.test(cleaned) ? cleaned : null;
}

function updateRestartBanner() {
    const banner = $('restart-banner');
    if (!banner) return;
    
    if (pendingRestart) {
        banner.style.display = 'flex';
        banner.classList.remove('opacity-0', 'translate-y-full');
        banner.classList.add('opacity-100', 'translate-y-0');
    } else {
        banner.classList.add('opacity-0', 'translate-y-full');
        banner.classList.remove('opacity-100', 'translate-y-0');
        setTimeout(() => {
            if (!pendingRestart) banner.style.display = 'none';
        }, 300);
    }
}

function showRestartModal() {
    $('restart-modal').classList.remove('hidden');
}

function hideRestartModal() {
    $('restart-modal').classList.add('hidden');
}

async function loadUserLists() {
    try {
        // Load include list
        const includeList = await invoke('read_user_list', { filename: 'list-general-user.txt' });
        renderList('site-include-list', includeList, 'list-general-user.txt');
        
        // Load exclude list
        const excludeList = await invoke('read_user_list', { filename: 'list-exclude-user.txt' });
        renderList('site-exclude-list', excludeList, 'list-exclude-user.txt');
        
        // Load IP exclude list
        const ipExcludeList = await invoke('read_user_list', { filename: 'ipset-exclude-user.txt' });
        renderList('ip-exclude-list', ipExcludeList, 'ipset-exclude-user.txt');
    } catch (err) {
        console.error('Error loading user lists:', err);
    }
}

function renderList(containerId, items, filename) {
    const container = $(containerId);
    container.innerHTML = '';
    
    items.forEach(item => {
        const row = document.createElement('div');
        row.className = 'flex items-center justify-between bg-surface-container-highest/50 rounded-xl px-4 py-2';
        row.innerHTML = `
            <span class="text-sm text-on-surface truncate">${escapeHtml(item)}</span>
            <button class="delete-btn text-error-dim hover:text-error-dim/70 transition-colors" data-item="${escapeHtml(item)}">
                <span class="material-symbols-outlined text-lg">delete</span>
            </button>
        `;
        
        row.querySelector('.delete-btn').addEventListener('click', async () => {
            try {
                await invoke('remove_from_user_list', { filename, entry: item });
                await loadUserLists();
                
                // Only require restart if service is running
                const status = await invoke('get_zapret_status');
                if (status.running) {
                    pendingRestart = true;
                    restartGuardDismissed = false;
                    updateRestartBanner();
                }
            } catch (err) {
                console.error('Error removing item:', err);
            }
        });
        
        container.appendChild(row);
    });
}

function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

async function addToList(inputId, filename) {
    const input = $(inputId);
    let value = input.value.trim();
    
    if (!value) return;

    let validatedValue = null;
    if (filename.includes('ipset')) {
        validatedValue = validateIP(value);
        if (!validatedValue) {
            input.classList.add('border-error-dim');
            setTimeout(() => input.classList.remove('border-error-dim'), 2000);
            return;
        }
    } else {
        validatedValue = cleanAndValidateDomain(value);
        if (!validatedValue) {
            input.classList.add('border-error-dim');
            setTimeout(() => input.classList.remove('border-error-dim'), 2000);
            return;
        }
    }
    
    try {
        await invoke('add_to_user_list', { filename, entry: validatedValue });
        input.value = '';
        await loadUserLists();

        // Only require restart if service is running
        const status = await invoke('get_zapret_status');
        if (status.running) {
            pendingRestart = true;
            restartGuardDismissed = false;
            updateRestartBanner();
        }
    } catch (err) {
        console.error('Error adding item:', err);
    }
}

window.addEventListener('DOMContentLoaded', async () => {
    initI18n();
    // Disable context menu (right-click)
    document.addEventListener('contextmenu', (e) => e.preventDefault());

    // ─── Custom title bar window controls ────────────────────────────────────
    try {
        const win = window.__TAURI__.window.getCurrentWindow();
        const tbMin   = $('tb-minimize');
        const tbClose = $('tb-close');
        if (tbMin)   tbMin.addEventListener('click',   () => win.minimize());
        if (tbClose) tbClose.addEventListener('click', () => win.close());
    } catch (e) {
        console.warn('Title bar controls unavailable:', e);
    }
    
    // Check admin privileges on startup
    try {
        const isAdmin = await invoke('check_admin_privileges');
        if (!isAdmin) {
            $('admin-check-modal').classList.remove('hidden');
            $('admin-check-close').addEventListener('click', () => {
                // Close the application
                if (window.__TAURI__.process) {
                    window.__TAURI__.process.exit(1);
                }
            });
            return; // Don't initialize the rest of the app
        }
    } catch (err) {
        console.error('Failed to check admin privileges:', err);
    }
    
    await loadStrategies();
    initStrategyDropdown();

    // Check if binaries are present (first launch)
    try {
        const binariesPresent = await invoke('ensure_binaries_present');
        if (!binariesPresent) {
            // Show first-launch modal and auto-download
            const modal = $('first-launch-modal');
            const statusEl = $('first-launch-status');
            const progressBar = $('first-launch-progress-bar');
            const progressText = $('first-launch-progress-text');
            
            if (modal) modal.classList.remove('hidden');
            if (statusEl) statusEl.textContent = t('initializing_download');

            listen('download-progress', (event) => {
                const pct = event.payload;
                if (progressBar) progressBar.style.width = pct + '%';
                if (progressText) progressText.textContent = pct + '%';
                if (statusEl && pct < 90) statusEl.textContent = t('downloading_core');
                if (statusEl && pct >= 90) statusEl.textContent = t('extracting');
            });

            try {
                await invoke('download_and_install_update');
                if (statusEl) statusEl.textContent = t('install_complete');
                if (progressBar) progressBar.style.width = '100%';
                if (progressText) progressText.textContent = '100%';
                await new Promise(r => setTimeout(r, 1000));
                location.reload();
            } catch(err) {
                if (statusEl) statusEl.textContent = t('download_failed') + ': ' + err + '\n\n' + t('restart_to_fix');
            }
            return; 
        }
    } catch (err) {
        console.error('Failed to check binaries:', err);
    }

    
    // Load versions dynamically
    try {
        const localVersion = await invoke('get_local_version_cmd');
        const uiVersion = await invoke('get_ui_version_cmd');
        
        const versionDisplays = ['version-display', 'latest-version-number', 'update-current-version'];
        versionDisplays.forEach(id => {
            const el = $(id);
            if (el) el.textContent = id === 'version-display' ? 'v' + localVersion : localVersion;
        });

        const uiEl = $('ui-version-display');
        if (uiEl) uiEl.textContent = 'v' + uiVersion;
    } catch (e) {
        console.error('Failed to get versions:', e);
    }

    // Сначала получаем статус — чтобы в dropdown сразу встала активная стратегия
    await pollStatus();
    await pollFilters();
    
    // Sync tray on startup
    syncTrayLocalization();

    // Поллинг каждые 2 секунды
    setInterval(async () => {
        await pollStatus();
        await pollFilters();
    }, 2000);

    // Navigation
    document.querySelector('aside nav a:first-child').addEventListener('click', (e) => {
        e.preventDefault();
        showSection('home');
    });
    $('nav-sites').addEventListener('click', (e) => {
        e.preventDefault();
        showSection('sites');
        loadUserLists();
    });
    $('nav-ips').addEventListener('click', (e) => {
        e.preventDefault();
        showSection('ips');
        loadUserLists();
    });
    $('nav-diagnostics').addEventListener('click', (e) => {
        e.preventDefault();
        showSection('diagnostics');
    });

    $('lang-switcher').addEventListener('click', toggleLanguage);

    $('connect-btn').addEventListener('click', handleConnectClick);
    const tempBtn = $('connect-temp-btn');
    if (tempBtn) {
        tempBtn.addEventListener('click', handleConnectClick);
    }

    // Слушатели для Game Filter
    $('game-toggle').addEventListener('click', () => {
        const isOn = currentFilters.game_filter !== 'disabled';
        if (isOn) {
            // Сохраняем текущее состояние и выключаем
            previousGameFilter = currentFilters.game_filter;
            handleGameFilterChange('disabled');
        } else {
            // Включаем с предыдущим состоянием
            handleGameFilterChange(previousGameFilter);
        }
    });
    $('game-all').addEventListener('click', () => handleGameFilterChange('all'));
    $('game-tcp').addEventListener('click', () => handleGameFilterChange('tcp'));
    $('game-udp').addEventListener('click', () => handleGameFilterChange('udp'));

    // Слушатели для IPSet Filter
    $('ipset-toggle').addEventListener('click', () => {
        console.log('IPSet toggle clicked, current state:', currentFilters.ipset);
        const isOn = currentFilters.ipset !== 'none';
        console.log('isOn:', isOn);
        if (isOn) {
            // Сохраняем текущее состояние и выключаем (none)
            previousIPSet = currentFilters.ipset;
            console.log('Turning OFF, saving previous:', previousIPSet);
            handleIPSetFilterChange('none');
        } else {
            // Включаем с предыдущим состоянием
            console.log('Turning ON, using previous:', previousIPSet);
            handleIPSetFilterChange(previousIPSet);
        }
    });
    $('ipset-loaded').addEventListener('click', () => {
        console.log('IPSet loaded button clicked');
        handleIPSetFilterChange('loaded');
    });
    $('ipset-any').addEventListener('click', () => {
        console.log('IPSet any button clicked');
        handleIPSetFilterChange('any');
    });

    // Info Modals Logic
    const infoModal = $('info-modal');
    const infoTitle = $('info-title');
    const infoContent = $('info-content');
    const infoClose = $('info-modal-close');

    let currentInfoType = null;

    const getInfoData = () => ({
        ipset: {
            title: t('ipset_info_title'),
            content: t('ipset_info_content')
        },
        game: {
            title: t('game_info_title'),
            content: t('game_info_content')
        },
        include: {
            title: t('include_info_title'),
            content: t('include_info_content')
        },
        exclude: {
            title: t('exclude_info_title'),
            content: t('exclude_info_content')
        },
        ip_exclude: {
            title: t('ip_exclude_info_title'),
            content: t('ip_exclude_info_content')
        }
    });

    const showInfo = (type) => {
        currentInfoType = type;
        const infoData = getInfoData();
        const data = infoData[type];
        if (!data) return;
        infoTitle.textContent = data.title;
        infoContent.innerHTML = data.content;
        infoModal.classList.remove('hidden');
    };

    window.refreshOpenInfoModal = () => {
        if (!infoModal.classList.contains('hidden') && currentInfoType) {
            showInfo(currentInfoType);
        }
    };

    $('ipset-info-btn').addEventListener('click', () => showInfo('ipset'));
    $('game-info-btn').addEventListener('click', () => showInfo('game'));
    const incInfo = $('include-info-btn');
    if (incInfo) incInfo.addEventListener('click', () => showInfo('include'));
    const excInfo = $('exclude-info-btn');
    if (excInfo) excInfo.addEventListener('click', () => showInfo('exclude'));
    const ipExcInfo = $('ip-exclude-info-btn');
    if (ipExcInfo) ipExcInfo.addEventListener('click', () => showInfo('ip_exclude'));

    infoClose.addEventListener('click', () => {
        infoModal.classList.add('hidden');
        currentInfoType = null;
    });
    infoModal.addEventListener('click', (e) => {
        if (e.target === infoModal) {
            infoModal.classList.add('hidden');
            currentInfoType = null;
        }
    });

    // User Lists - sites
    const bindAddList = (btnId, inputId, filename) => {
        const btn = $(btnId);
        const input = $(inputId);
        if (btn) btn.onclick = () => addToList(inputId, filename);
        if (input) {
            input.onkeypress = (e) => {
                if (e.key === 'Enter') addToList(inputId, filename);
            };
        }
    };

    bindAddList('site-include-add', 'site-include-input', 'list-general-user.txt');
    bindAddList('site-exclude-add', 'site-exclude-input', 'list-exclude-user.txt');
    bindAddList('ip-exclude-add', 'ip-exclude-input', 'ipset-exclude-user.txt');

    // Restart Modal
    $('restart-later').addEventListener('click', () => {
        hideRestartModal();
        restartGuardDismissed = true;
        // Proceed with navigation if the modal was triggered by a guard
        const lastNavId = window.pendingNavId;
        if (lastNavId) {
            window.pendingNavId = null;
            showSection(lastNavId);
        }
    });

    $('restart-now').addEventListener('click', async () => {
        hideRestartModal();
        if (pendingRestart) {
            await restartServiceIfRunning();
            pendingRestart = false;
            updateRestartBanner();
        }
        // Proceed with navigation if the modal was triggered by a guard
        const lastNavId = window.pendingNavId;
        if (lastNavId) {
            window.pendingNavId = null;
            showSection(lastNavId);
        }
    });

    // Global Restart Banner Button
    $('restart-banner-btn').addEventListener('click', async () => {
        if (pendingRestart) {
            await restartServiceIfRunning();
            pendingRestart = false;
            updateRestartBanner();
        }
    });

    // Update IPSet List button
    const ipsetUpdateBtn = $('ipset-update-btn');
    if (ipsetUpdateBtn) {
        ipsetUpdateBtn.addEventListener('click', async () => {
            const statusEl = $('ipset-update-status');
            statusEl.classList.remove('hidden');
            statusEl.textContent = t('updating');
            statusEl.className = 'mt-4 text-sm text-secondary';
            ipsetUpdateBtn.disabled = true;
            
            try {
                const result = await invoke('update_ipset_list');
                // result expected to be something like "Updated successfully. 15993 IPs loaded."
                // Since it's from backend, we might want to try to parse the count if we want full localization, 
                // but for now let's just use the translated string if we can.
                // If it's a fixed format, we can parse it.
                const countMatch = result.match(/\d+/);
                const count = countMatch ? countMatch[0] : '?';
                
                statusEl.textContent = t('update_success', { count });
                statusEl.className = 'mt-4 text-sm text-secondary';
                
                // Only require restart if service is running
                const status = await invoke('get_zapret_status');
                if (status.running) {
                    pendingRestart = true;
                    restartGuardDismissed = false;
                    updateRestartBanner();
                }
            } catch (err) {
                statusEl.textContent = 'Error: ' + err;
                statusEl.className = 'mt-4 text-sm text-error-dim';
            } finally {
                ipsetUpdateBtn.disabled = false;
            }
        });
    }

    // Check for Updates functionality
    const checkUpdatesBtn = $('check-updates-btn');
    const updateModal = $('update-modal');
    
    function showUpdateModal(result) {
        $('update-current-version').textContent = result.current_version;
        if (result.latest_version) {
            $('update-latest-version').textContent = result.latest_version;
        }
        updateModal.classList.remove('hidden');
    }
    
    function hideUpdateModal() {
        updateModal.classList.add('hidden');
        $('update-status').classList.add('hidden');
    }
    
    const latestVersionModal = $('latest-version-modal');
    
    function showLatestVersionModal(version) {
        $('latest-version-number').textContent = version;
        latestVersionModal.classList.remove('hidden');
    }
    
    function hideLatestVersionModal() {
        latestVersionModal.classList.add('hidden');
    }
    
    async function checkForUpdates(manual = false) {
        if (!window.__TAURI__) return;
        const { check } = window.__TAURI__.updater;

        if (manual && checkUpdatesBtn) {
            checkUpdatesBtn.disabled = true;
            checkUpdatesBtn.innerHTML = `<span class="material-symbols-outlined text-sm animate-spin">refresh</span> ${t('updating')}`;
        }
        
        try {
            // Get current local UI version from the DOM or default
            const uiLocalVersion = ($('ui-version-display')?.textContent || '0.1.0').replace('v', '');

            // Run both checks in parallel
            const [uiUpdate, coreRemoteVersion, coreLocalVersion] = await Promise.all([
                check().catch(err => {
                    console.warn('UI update check failed (normal in dev):', err);
                    return null;
                }),
                invoke('get_remote_core_version').catch((err) => 'Remote Err: ' + err),
                invoke('get_local_version_cmd').catch((err) => 'Local Err: ' + err)
            ]);
            
            const hasUIUpdate = !!uiUpdate;
            const hasCoreUpdate = coreRemoteVersion !== 'Unknown' && coreLocalVersion !== 'Unknown' && coreRemoteVersion !== coreLocalVersion;
            
            if (hasUIUpdate || hasCoreUpdate || manual) {
                showDualUpdateModal({
                    ui: {
                        available: hasUIUpdate,
                        current: uiLocalVersion,
                        latest: hasUIUpdate ? uiUpdate.version : uiLocalVersion,
                        updateObj: uiUpdate
                    },
                    core: {
                        available: hasCoreUpdate,
                        current: coreLocalVersion,
                        latest: coreRemoteVersion
                    }
                }, manual);
            }
        } catch (err) {
            console.error('Error checking for updates:', err);
            if (manual) showDualUpdateModal(null, true);
        } finally {
            if (manual && checkUpdatesBtn) {
                checkUpdatesBtn.disabled = false;
                checkUpdatesBtn.innerHTML = `<span class="material-symbols-outlined text-sm">update</span> <span data-i18n="check_updates">${t('check_updates')}</span>`;
            }
        }
    }

    async function downloadAndInstallCoreUpdate() {
        try {
            const modalTitle = document.querySelector('#update-modal h3');
            if (modalTitle) modalTitle.textContent = t('downloading_installing');
            
            await invoke('download_and_install_update');
            
            if (modalTitle) modalTitle.textContent = t('update_installed_restarting');
            setTimeout(() => location.reload(), 1500);
        } catch (err) {
            console.error('Core update failed:', err);
            alert('Core update failed: ' + err);
        }
    }

    function showDualUpdateModal(data, manual = false) {
        // Clean up previous modal
        const oldModal = $('update-modal');
        if (oldModal) oldModal.remove();

        if (!data && manual) {
            // Fallback object if we couldn't fetch anything but were triggered manually
            const currentUI = ($('ui-version-display')?.textContent || '0.1.0').replace('v', '');
            data = {
                ui: { available: false, current: currentUI, latest: currentUI },
                core: { available: false, current: 'Unknown', latest: 'Unknown' }
            };
        }

        const modal = document.createElement('div');
        modal.id = 'update-modal';
        modal.className = 'fixed inset-0 z-[1000] flex items-center justify-center p-6 bg-background/80 backdrop-blur-md animate-fade-in';
        
        const uiStatus = data.ui.available ? 
            `<span class="px-2 py-0.5 bg-primary/20 text-primary text-[10px] font-bold rounded-full uppercase">${t('update_available_short')}</span>` : 
            `<span class="text-on-surface-variant/50 text-[10px] font-bold uppercase">${t('up_to_date')}</span>`;
            
        const coreStatus = data.core.available ? 
            `<span class="px-2 py-0.5 bg-secondary/20 text-secondary text-[10px] font-bold rounded-full uppercase">${t('update_available_short')}</span>` : 
            `<span class="text-on-surface-variant/50 text-[10px] font-bold uppercase">${t('up_to_date')}</span>`;

        modal.innerHTML = `
            <div class="bg-surface-container-high border border-outline-variant/30 rounded-3xl p-8 max-w-lg w-full shadow-2xl animate-scale-in">
                <div class="flex flex-col items-center">
                    <div class="w-16 h-16 bg-primary/10 rounded-2xl flex items-center justify-center mb-6">
                        <span class="material-symbols-outlined text-3xl text-primary">system_update_alt</span>
                    </div>
                    <h3 class="font-headline text-2xl font-black text-on-surface mb-6 uppercase tracking-tight">${t('check_updates')}</h3>
                    
                    <div class="w-full space-y-3 mb-8">
                        <!-- Application UI Row -->
                        <div class="flex items-center justify-between p-4 bg-white/5 rounded-2xl border border-white/5">
                            <div class="flex flex-col items-start text-left">
                                <span class="text-[10px] font-bold text-primary/70 uppercase tracking-wider mb-1">${t('app_ui')}</span>
                                <div class="flex items-center gap-2">
                                    <span class="text-sm font-bold text-on-surface">v${data.ui.current}</span>
                                    ${data.ui.available ? `<span class="material-symbols-outlined text-xs text-on-surface-variant/40">arrow_forward</span> <span class="text-sm font-bold text-primary">v${data.ui.latest}</span>` : ''}
                                </div>
                            </div>
                            <div class="flex flex-col items-end gap-2">
                                ${uiStatus}
                                ${data.ui.available ? `<button class="text-[11px] font-black text-primary uppercase hover:underline" onclick="window.downloadAndInstallUIUpdate(window.currentUpdateObject)">${t('update')}</button>` : ''}
                            </div>
                        </div>

                        <!-- Zapret Core Row -->
                        <div class="flex items-center justify-between p-4 bg-white/5 rounded-2xl border border-white/5">
                            <div class="flex flex-col items-start text-left">
                                <span class="text-[10px] font-bold text-secondary/70 uppercase tracking-wider mb-1">${t('zapret_core')}</span>
                                <div class="flex items-center gap-2">
                                    <span class="text-sm font-bold text-on-surface">v${data.core.current}</span>
                                    ${data.core.available ? `<span class="material-symbols-outlined text-xs text-on-surface-variant/40">arrow_forward</span> <span class="text-sm font-bold text-secondary">v${data.core.latest}</span>` : ''}
                                </div>
                            </div>
                            <div class="flex flex-col items-end gap-2">
                                ${coreStatus}
                                ${data.core.available ? `<button class="text-[11px] font-black text-secondary uppercase hover:underline" onclick="window.downloadAndInstallCoreUpdate()">${t('update')}</button>` : ''}
                            </div>
                        </div>
                    </div>

                    <button class="w-full px-4 py-3 bg-white/5 text-on-surface-variant rounded-xl font-bold hover:bg-white/10 transition-all uppercase text-xs tracking-widest" onclick="this.closest('#update-modal').remove()">
                        ${t('close')}
                    </button>
                </div>
            </div>
        `;
        window.currentUpdateObject = data.ui.updateObj;
        document.body.appendChild(modal);
    }
    
    window.downloadAndInstallCoreUpdate = downloadAndInstallCoreUpdate;
    
    // Auto-check on startup (after a short delay)
    setTimeout(() => checkForUpdates(false), 3000);
    
    // Manual check button
    if (checkUpdatesBtn) {
        checkUpdatesBtn.addEventListener('click', () => checkForUpdates(true));
    }
    
    $('update-now').addEventListener('click', async () => {
        const statusEl = $('update-status');
        const updateNowBtn = $('update-now');

        statusEl.classList.remove('hidden');
        statusEl.className = 'mt-4 text-sm text-secondary';
        updateNowBtn.disabled = true;

        let zapretWasRunning = false;
        let zapretStrategy = null;
        let zapretMode = 'service';

        try {
            // 1. Check if zapret is currently running
            statusEl.textContent = t('checking_service_status');
            const status = await invoke('get_zapret_status');
            if (status.running) {
                zapretWasRunning = true;
                zapretStrategy = status.strategy;
                zapretMode = status.mode || 'service';

                statusEl.textContent = t('stopping_before_update');
                await invoke('stop_zapret');
            }

            // 2. Download and install
            const progressContainer = $('update-status-container');
            const progressText = $('update-progress-text');
            const progressBar = $('update-progress-bar');
            
            if (progressContainer) {
                progressContainer.classList.remove('hidden');
                statusEl.textContent = t('downloading_installing');
            }

            const unlisten = await listen('download-progress', (event) => {
                const pct = event.payload;
                if (progressBar) progressBar.style.width = pct + '%';
                if (progressText) progressText.textContent = pct + '%';
                if (statusEl && pct >= 90) statusEl.textContent = t('extracting_installing');
            });

            const result = await invoke('download_and_install_update');
            if (unlisten) unlisten();

            if (progressBar) progressBar.style.width = '100%';
            if (progressText) progressText.textContent = '100%';
            statusEl.className = 'text-xs text-secondary font-mono mb-3 text-center';

            // 3. Restart if was running
            if (zapretWasRunning && zapretStrategy) {
                statusEl.textContent = t('update_installed_restarting');
                try {
                    await invoke('start_zapret', { strategy: zapretStrategy, mode: zapretMode });
                    await pollStatus();
                    statusEl.textContent = result + ' Zapret restarted successfully.';
                } catch (restartErr) {
                    statusEl.textContent = result + ' Warning: failed to restart: ' + restartErr;
                    statusEl.className = 'text-xs text-primary font-mono mb-3 text-center';
                }
            } else {
                statusEl.textContent = result;
            }

            // Update the local version string on UI immediately
            try {
                const refreshedVersion = await invoke('get_local_version_cmd');
                const versionDisplays = ['version-display', 'latest-version-number', 'update-current-version'];
                versionDisplays.forEach(id => {
                    const el = $(id);
                    if (el) el.textContent = id === 'version-display' ? 'v' + refreshedVersion : refreshedVersion;
                });
            } catch (e) {
                console.error("Failed to refresh version:", e);
            }

            updateNowBtn.textContent = 'Done';
            updateNowBtn.disabled = false;
            updateNowBtn.onclick = () => location.reload(); // Reload after update to refresh versions


        } catch (err) {
            statusEl.textContent = 'Error: ' + err;
            statusEl.className = 'mt-4 text-sm text-error-dim';

            // Try to restore zapret even if update failed
            if (zapretWasRunning && zapretStrategy) {
                try {
                    await invoke('start_zapret', { strategy: zapretStrategy, mode: zapretMode });
                    await pollStatus();
                } catch (_) {}
            }

            updateNowBtn.disabled = false;
        }
    });

    // Diagnostics functionality
    const runDiagnosticsBtn = $('run-diagnostics-btn');
    const diagnosticsResults = $('diagnostics-results');
    const discordCacheSection = $('discord-cache-section');
    const showAllBtn = $('diagnostics-show-all-btn');
    
    let lastDiagnosticsResults = null;
    let showingAllDiagnostics = false;

    function renderDiagnostics(result, showAll) {
        diagnosticsResults.innerHTML = '';
        if (!result || !result.checks) return;

        let hiddenCount = 0;
        
        result.checks.forEach(check => {
            const isSuccess = check.status === 'passed';
            if (!showAll && isSuccess) {
                hiddenCount++;
                return;
            }

            const row = document.createElement('div');
            row.className = 'bg-white/5 rounded-xl border p-4 flex items-start gap-3 transition-opacity duration-300';
            
            let icon, iconColor, borderColor;
            if (isSuccess) {
                icon = 'check_circle';
                iconColor = 'text-secondary';
                borderColor = 'border-secondary/30';
            } else if (check.status === 'warning') {
                icon = 'warning';
                iconColor = 'text-primary';
                borderColor = 'border-primary/30';
            } else {
                icon = 'error';
                iconColor = 'text-error-dim';
                borderColor = 'border-error-dim/30';
            }
            
            row.classList.add(borderColor);
            
            let linkHtml = '';
            if (check.link) {
                linkHtml = `<a href="${check.link}" target="_blank" class="text-xs text-primary hover:underline mt-1 block">${check.link}</a>`;
            }
            
            row.innerHTML = `
                <span class="material-symbols-outlined ${iconColor} text-xl mt-0.5">${icon}</span>
                <div class="flex-1">
                    <h4 class="font-headline text-sm font-bold text-on-surface">${check.name}</h4>
                    <p class="text-xs text-on-surface-variant mt-1">${check.message}</p>
                    ${linkHtml}
                </div>
            `;
            
            diagnosticsResults.appendChild(row);
        });

        // Show/hide the "Show All" toggle button
        if (hiddenCount > 0 || showAll) {
            showAllBtn.classList.remove('hidden');
            showAllBtn.textContent = showAll ? 'Hide Successful Checks' : `Show All Checks (${hiddenCount} hidden)`;
        } else {
            showAllBtn.classList.add('hidden');
        }

        // Show VPN services if found
        if (result.vpn_services) {
            const vpnRow = document.createElement('div');
            vpnRow.className = 'bg-white/5 rounded-xl border border-primary/30 p-4 mt-3';
            vpnRow.innerHTML = `
                <div class="flex items-start gap-3">
                    <span class="material-symbols-outlined text-primary text-xl mt-0.5">vpn_key</span>
                    <div class="flex-1">
                        <h4 class="font-headline text-sm font-bold text-on-surface">VPN Services Found</h4>
                        <p class="text-xs text-on-surface-variant mt-1">${result.vpn_services}</p>
                        <p class="text-xs text-primary mt-2">Make sure that all VPNs are disabled</p>
                    </div>
                </div>
            `;
            diagnosticsResults.appendChild(vpnRow);
        }
    }

    if (runDiagnosticsBtn) {
        runDiagnosticsBtn.addEventListener('click', async () => {
            runDiagnosticsBtn.disabled = true;
            runDiagnosticsBtn.innerHTML = '<span class="material-symbols-outlined text-sm animate-spin">refresh</span> Running...';
            diagnosticsResults.innerHTML = '';
            diagnosticsResults.classList.remove('hidden');
            discordCacheSection.classList.add('hidden');
            showAllBtn.classList.add('hidden');
            showingAllDiagnostics = false;
            
            try {
                const result = await invoke('run_diagnostics');
                lastDiagnosticsResults = result;
                renderDiagnostics(result, false);
                
                // Show Discord cache clear option
                discordCacheSection.classList.remove('hidden');
                
            } catch (err) {
                diagnosticsResults.innerHTML = `
                    <div class="bg-white/5 rounded-xl border border-error-dim/30 p-4 text-error-dim text-sm">
                        Failed to run diagnostics: ${err}
                    </div>
                `;
            } finally {
                runDiagnosticsBtn.disabled = false;
                runDiagnosticsBtn.innerHTML = '<span class="material-symbols-outlined text-sm">play_arrow</span> Run Diagnostics';
            }
        });
    }

    if (showAllBtn) {
        showAllBtn.addEventListener('click', () => {
            showingAllDiagnostics = !showingAllDiagnostics;
            renderDiagnostics(lastDiagnosticsResults, showingAllDiagnostics);
        });
    }
    
    // Clear Discord cache
    const clearDiscordCacheBtn = $('clear-discord-cache-btn');
    if (clearDiscordCacheBtn) {
        clearDiscordCacheBtn.addEventListener('click', async () => {
            const statusEl = $('discord-cache-status');
            statusEl.classList.remove('hidden');
            statusEl.innerHTML = 'Clearing...';
            statusEl.className = 'mt-4 text-sm text-secondary whitespace-pre-line';
            clearDiscordCacheBtn.disabled = true;
            
            try {
                const result = await invoke('clear_discord_cache');
                // Convert newlines to <br> for HTML display
                statusEl.innerHTML = escapeHtml(result).replace(/\n/g, '<br>');
                statusEl.className = 'mt-4 text-sm text-secondary whitespace-pre-line';
            } catch (err) {
                statusEl.textContent = 'Error: ' + err;
                statusEl.className = 'mt-4 text-sm text-error-dim';
            } finally {
                clearDiscordCacheBtn.disabled = false;
            }
        });
    }

    // Run Tests functionality
    const runTestsBtn = $('run-tests-btn');
    const cancelTestsBtn = $('cancel-tests-btn');
    const testsStatus = $('tests-status');
    const testsLog = $('tests-log');
    const testsResults = $('tests-results');
    let testsRunning = false;
    let selectedTestType = 'standard';

    // Test type toggle
    document.querySelectorAll('[data-type]').forEach(btn => {
        btn.addEventListener('click', () => {
            if (testsRunning) return;
            selectedTestType = btn.dataset.type;
            document.querySelectorAll('[data-type]').forEach(b => {
                b.classList.remove('card-active', 'text-on-background');
                b.classList.add('text-on-surface-variant', 'border-outline-variant/30');
                b.classList.remove('border-primary/30');
            });
            btn.classList.add('card-active', 'border-primary/30');
            btn.classList.remove('text-on-surface-variant', 'border-outline-variant/30');
        });
    });

    // Color map for log lines
    const logColors = {
        error:   'text-error-dim',
        warning: 'text-primary',
        success: 'text-secondary',
        separator: 'text-on-surface-variant',
        config:  'text-secondary font-bold',
        info:    'text-on-surface/80',
    };

    function appendLog(line, kind) {
        const el = document.createElement('div');
        el.className = logColors[kind] || 'text-on-surface/80';
        el.textContent = line;
        testsLog.appendChild(el);
        // Auto-scroll to bottom
        testsLog.scrollTop = testsLog.scrollHeight;
    }
    
    if (runTestsBtn) {
        runTestsBtn.addEventListener('click', async () => {
            if (testsRunning) return;
            
            testsRunning = true;
            runTestsBtn.disabled = true;
            runTestsBtn.innerHTML = '<span class="material-symbols-outlined text-sm animate-spin">refresh</span> Testing...';
            cancelTestsBtn.classList.remove('hidden');

            testsStatus.classList.remove('hidden');
            testsStatus.textContent = t('test_running_info', { type: selectedTestType === 'dpi' ? 'DPI' : 'Standard' });
            testsStatus.className = 'text-sm mb-3 text-primary';

            // Clear previous
            testsLog.innerHTML = '';
            testsLog.classList.remove('hidden');
            testsResults.innerHTML = '';
            testsResults.classList.add('hidden');

            // Update main header status to Testing...
            $('hero-status').textContent = t('testing');
            $('hero-status').className = 'text-primary';
            $('header-status').innerHTML = `<span class="text-primary"><span data-i18n="status_label">${t('status_label')}</span>:</span> <span class="text-primary" data-i18n="testing">${t('testing')}</span>`;

            // Subscribe to streaming events
            let unlistenProgress, unlistenDone;
            unlistenProgress = await window.__TAURI__.event.listen('test-progress', (event) => {
                const { line, kind } = event.payload;
                appendLog(line, kind);
            });
            unlistenDone = await window.__TAURI__.event.listen('test-done', () => {
                if (unlistenProgress) unlistenProgress();
                if (unlistenDone) unlistenDone();
            });
            
            try {
                const results = await invoke('run_tests', {
                    testType: selectedTestType,
                    testMode: 'all'
                });
                
                testsStatus.textContent = `Tests completed. ${results.length} configurations tested.`;
                testsStatus.className = 'text-sm mb-3 text-secondary';

                if (results.length > 0) {
                    testsResults.classList.remove('hidden');

                    // Best strategy
                    let bestStrategy = null;
                    let bestScore = -Infinity;
                    results.forEach(r => {
                        const score = r.http_ok + r.ping_ok - r.http_error * 2 - r.ping_fail;
                        if (score > bestScore) { bestScore = score; bestStrategy = r; }
                    });

                    window.downloadAndInstallCoreUpdate = downloadAndInstallCoreUpdate;

                    if (bestStrategy) {
                        const bestRow = document.createElement('div');
                        bestRow.className = 'glass-panel rounded-xl border border-secondary/50 p-4 bg-secondary/5';
                        bestRow.innerHTML = `
                            <div class="flex items-center gap-3">
                                <span class="material-symbols-outlined text-secondary text-xl">trophy</span>
                                <div>
                                    <h4 class="font-headline text-sm font-bold text-secondary">Best Strategy</h4>
                                    <p class="text-xs text-on-surface-variant mt-1">${bestStrategy.config.replace('.bat', '')}</p>
                                </div>
                            </div>
                        `;
                        testsResults.appendChild(bestRow);
                    }

                    results.forEach(result => {
                        const row = document.createElement('div');
                        const isBest = bestStrategy && result.config === bestStrategy.config;
                        let borderColor = result.status === 'success' ? 'border-secondary/30' : result.status === 'partial' ? 'border-primary/30' : 'border-error-dim/30';
                        let icon = result.status === 'success' ? 'check_circle' : result.status === 'partial' ? 'warning' : 'error';
                        let iconColor = result.status === 'success' ? 'text-secondary' : result.status === 'partial' ? 'text-primary' : 'text-error-dim';
                        row.className = `glass-panel rounded-xl border ${borderColor} p-3 flex items-center justify-between`;
                        row.innerHTML = `
                            <div class="flex items-center gap-2">
                                <span class="material-symbols-outlined ${iconColor} text-base">${icon}</span>
                                <span class="font-headline text-xs font-bold text-on-surface">${result.config.replace('.bat', '')}</span>
                                ${isBest ? '<span class="text-[9px] bg-secondary/20 text-secondary px-2 py-0.5 rounded-full uppercase tracking-wider">Best</span>' : ''}
                            </div>
                            <div class="text-[10px] text-on-surface-variant text-right">
                                HTTP: <span class="text-secondary">${result.http_ok}✓</span><span class="text-error-dim">${result.http_error > 0 ? ' ' + result.http_error + '✗' : ''}</span>
                                Ping: <span class="text-secondary">${result.ping_ok}✓</span><span class="text-error-dim">${result.ping_fail > 0 ? ' ' + result.ping_fail + '✗' : ''}</span>
                            </div>
                        `;
                        testsResults.appendChild(row);
                    });
                }

            } catch (err) {
                if (unlistenProgress) unlistenProgress();
                if (unlistenDone) unlistenDone();
                testsStatus.textContent = 'Error: ' + err;
                testsStatus.className = 'text-sm mb-3 text-error-dim';
            } finally {
                testsRunning = false;
                runTestsBtn.disabled = false;
                runTestsBtn.innerHTML = '<span class="material-symbols-outlined text-sm">science</span> Run Tests';
                cancelTestsBtn.classList.add('hidden');
                await pollStatus();
            }
        });
    }
    
    if (cancelTestsBtn) {
        cancelTestsBtn.addEventListener('click', async () => {
            testsStatus.textContent = 'Cancelling...';
            cancelTestsBtn.disabled = true;
            try {
                await invoke('cancel_tests');
            } catch (err) {
                console.error('Cancel error:', err);
            }
        });
    }

    const checkStatusBtn = $('check-status-btn');
    const statusModal = $('status-modal');
    const statusContent = $('status-content');
    const statusModalClose = $('status-modal-close');

    if (checkStatusBtn && statusModal && statusContent) {
        checkStatusBtn.addEventListener('click', async () => {
            statusContent.textContent = 'Checking status in real-time...';
            statusModal.classList.remove('hidden');
            try {
                const status = await invoke('check_status_full');
                statusContent.textContent = status;
            } catch (err) {
                statusContent.textContent = 'Error checking status: ' + err;
            }
        });
    }

    if (statusModalClose && statusModal) {
        statusModalClose.addEventListener('click', () => {
            statusModal.classList.add('hidden');
        });
    }
});
