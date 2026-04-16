// invoke получаем лениво, чтобы не было гонки с инициализацией Tauri
function invoke(cmd, args) {
    return window.__TAURI__.core.invoke(cmd, args);
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
            $('strategy-label').textContent = 'No strategies found';
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
        $('strategy-label').textContent = 'Error: ' + err;
    }
}

// ─── Статус zapret ────────────────────────────────────────────────────────────

function updateStatusUI(status) {
    const trigger = $('strategy-trigger');
    const tempBtn = $('connect-temp-btn');

    if (status.running) {
        const label = status.strategy ?? 'Connected';

        $('header-status').textContent = `Status: ${label}`;
        $('header-status').className =
            'text-[10px] font-bold uppercase tracking-[0.2em] opacity-80 text-secondary';

        $('hero-status').textContent = 'Connected';
        $('hero-status').className = 'text-secondary';

        $('connect-btn-text').textContent = 'Disconnect';
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
        if (trigger) trigger.disabled = true;

    } else {
        $('header-status').textContent = 'Status: Disconnected';
        $('header-status').className =
            'text-[10px] text-error-dim font-bold uppercase tracking-[0.2em] opacity-80';

        $('hero-status').textContent = 'Disconnected';
        $('hero-status').className = 'text-error-dim';

        $('connect-btn-text').textContent = 'Run as Service';
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
    $('hero-status').textContent = message;
    if (isRestarting) {
        $('hero-status').className = 'text-primary';
        $('header-status').textContent = 'Status: Restarting...';
        $('header-status').className = 'text-[10px] font-bold uppercase tracking-[0.2em] opacity-80 text-primary';
    } else {
        $('hero-status').className = 'text-secondary';
    }
}

// Функция для перезапуска сервиса если он запущен
async function restartServiceIfRunning() {
    const status = await invoke('get_zapret_status');
    if (status.running && status.strategy) {
        showRestartStatus('Restarting...', true);
        try {
            await invoke('stop_zapret');
            // Небольшая задержка для корректной остановки
            await new Promise(r => setTimeout(r, 1000));
            await invoke('start_zapret', { strategy: status.strategy, mode: status.mode || 'service' });
            showRestartStatus('Connected');
            await pollStatus();
            setTimeout(() => pollStatus(), 2000);
        } catch (err) {
            console.error('Ошибка перезапуска:', err);
            showRestartStatus('Restart failed: ' + err);
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
            $('hero-status').textContent = 'Starting service...';
            $('hero-status').className = 'text-secondary';
            await invoke('start_zapret', { strategy, mode });
            $('hero-status').textContent = 'Service started';
        } else {
            $('hero-status').textContent = 'Stopping...';
            await invoke('stop_zapret');
            $('hero-status').textContent = 'Disconnected';
        }
        await pollStatus();
    } catch (err) {
        console.error('Ошибка действия:', err);
        $('hero-status').textContent = `Error: ${err}`;
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
                pendingRestart = true;
                showRestartModal();
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
    const value = input.value.trim();
    
    if (!value) return;
    
    try {
        await invoke('add_to_user_list', { filename, entry: value });
        input.value = '';
        await loadUserLists();
        pendingRestart = true;
        showRestartModal();
    } catch (err) {
        console.error('Error adding item:', err);
    }
}

window.addEventListener('DOMContentLoaded', async () => {
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

    // User Lists - Sites
    $('site-include-add').addEventListener('click', () => addToList('site-include-input', 'list-general-user.txt'));
    $('site-include-input').addEventListener('keypress', (e) => {
        if (e.key === 'Enter') addToList('site-include-input', 'list-general-user.txt');
    });
    $('site-exclude-add').addEventListener('click', () => addToList('site-exclude-input', 'list-exclude-user.txt'));
    $('site-exclude-input').addEventListener('keypress', (e) => {
        if (e.key === 'Enter') addToList('site-exclude-input', 'list-exclude-user.txt');
    });

    // User Lists - IPs
    $('ip-exclude-add').addEventListener('click', () => addToList('ip-exclude-input', 'ipset-exclude-user.txt'));
    $('ip-exclude-input').addEventListener('keypress', (e) => {
        if (e.key === 'Enter') addToList('ip-exclude-input', 'ipset-exclude-user.txt');
    });

    // Restart Modal
    $('restart-later').addEventListener('click', () => {
        hideRestartModal();
    });
    $('restart-now').addEventListener('click', async () => {
        hideRestartModal();
        if (pendingRestart) {
            await restartServiceIfRunning();
            pendingRestart = false;
        }
    });

    // Update IPSet List button
    const ipsetUpdateBtn = $('ipset-update-btn');
    if (ipsetUpdateBtn) {
        ipsetUpdateBtn.addEventListener('click', async () => {
            const statusEl = $('ipset-update-status');
            statusEl.classList.remove('hidden');
            statusEl.textContent = 'Updating...';
            statusEl.className = 'mt-4 text-sm text-secondary';
            ipsetUpdateBtn.disabled = true;
            
            try {
                const result = await invoke('update_ipset_list');
                statusEl.textContent = result;
                statusEl.className = 'mt-4 text-sm text-secondary';
                pendingRestart = true;
                showRestartModal();
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
        if (manual && checkUpdatesBtn) {
            checkUpdatesBtn.disabled = true;
            checkUpdatesBtn.innerHTML = '<span class="material-symbols-outlined text-sm animate-spin">refresh</span> Checking...';
        }
        
        try {
            const result = await invoke('check_for_updates');
            
            // Update version display
            if ($('version-display')) {
                $('version-display').textContent = 'v' + result.current_version;
            }
            
            if (result.has_update) {
                showUpdateModal(result);
            } else if (manual) {
                // Show "latest version installed" modal
                showLatestVersionModal(result.current_version);
            }
        } catch (err) {
            console.error('Error checking for updates:', err);
            if (manual) {
                // Show error in UI instead of alert
                showLatestVersionModal('Unknown');
            }
        } finally {
            if (manual && checkUpdatesBtn) {
                checkUpdatesBtn.disabled = false;
                checkUpdatesBtn.innerHTML = '<span class="material-symbols-outlined text-sm">update</span> Check Updates';
            }
        }
    }
    
    // Auto-check on startup (after a short delay)
    setTimeout(() => checkForUpdates(false), 3000);
    
    // Manual check button
    if (checkUpdatesBtn) {
        checkUpdatesBtn.addEventListener('click', () => checkForUpdates(true));
    }
    
    // Update modal buttons
    $('update-later').addEventListener('click', hideUpdateModal);
    $('latest-version-ok').addEventListener('click', hideLatestVersionModal);
    
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
            statusEl.textContent = 'Checking service status...';
            const status = await invoke('get_zapret_status');
            if (status.running) {
                zapretWasRunning = true;
                zapretStrategy = status.strategy;
                zapretMode = status.mode || 'service';

                statusEl.textContent = 'Stopping zapret before update...';
                await invoke('stop_zapret');
            }

            // 2. Download and install
            statusEl.textContent = 'Downloading and installing update...';
            const result = await invoke('download_and_install_update');

            statusEl.className = 'mt-4 text-sm text-secondary';

            // 3. Restart if was running
            if (zapretWasRunning && zapretStrategy) {
                statusEl.textContent = 'Update installed. Restarting zapret...';
                try {
                    await invoke('start_zapret', { strategy: zapretStrategy, mode: zapretMode });
                    await pollStatus();
                    statusEl.textContent = result + ' Zapret restarted successfully.';
                } catch (restartErr) {
                    statusEl.textContent = result + ' Warning: failed to restart zapret: ' + restartErr;
                    statusEl.className = 'mt-4 text-sm text-primary';
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
            updateNowBtn.onclick = () => hideUpdateModal();

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
    
    if (runDiagnosticsBtn) {
        runDiagnosticsBtn.addEventListener('click', async () => {
            runDiagnosticsBtn.disabled = true;
            runDiagnosticsBtn.innerHTML = '<span class="material-symbols-outlined text-sm animate-spin">refresh</span> Running...';
            diagnosticsResults.innerHTML = '';
            diagnosticsResults.classList.remove('hidden');
            discordCacheSection.classList.add('hidden');
            
            try {
                const result = await invoke('run_diagnostics');
                
                // Render check results
                result.checks.forEach(check => {
                    const row = document.createElement('div');
                    row.className = 'glass-panel rounded-xl border p-4 flex items-start gap-3';
                    
                    let icon, iconColor, borderColor;
                    if (check.status === 'passed') {
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
                
                // Show VPN services if found
                if (result.vpn_services) {
                    const vpnRow = document.createElement('div');
                    vpnRow.className = 'glass-panel rounded-xl border border-primary/30 p-4 mt-3';
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
                
                // Show Discord cache clear option
                discordCacheSection.classList.remove('hidden');
                
            } catch (err) {
                diagnosticsResults.innerHTML = `
                    <div class="glass-panel rounded-xl border border-error-dim/30 p-4 text-error-dim">
                        Failed to run diagnostics: ${err}
                    </div>
                `;
            } finally {
                runDiagnosticsBtn.disabled = false;
                runDiagnosticsBtn.innerHTML = '<span class="material-symbols-outlined text-sm">play_arrow</span> Run Diagnostics';
            }
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
            testsStatus.textContent = `Running ${selectedTestType === 'dpi' ? 'DPI' : 'Standard'} tests. This may take several minutes...`;
            testsStatus.className = 'text-sm mb-3 text-primary';

            // Clear previous
            testsLog.innerHTML = '';
            testsLog.classList.remove('hidden');
            testsResults.innerHTML = '';
            testsResults.classList.add('hidden');

            // Update main header status to Testing...
            $('hero-status').textContent = 'Testing...';
            $('hero-status').className = 'text-primary';
            $('header-status').textContent = 'Status: Testing...';
            $('header-status').className = 'text-[10px] font-bold uppercase tracking-[0.2em] opacity-80 text-primary';

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
