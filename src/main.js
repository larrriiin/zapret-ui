// invoke получаем лениво, чтобы не было гонки с инициализацией Tauri
function invoke(cmd, args) {
    return window.__TAURI__.core.invoke(cmd, args);
}

const $ = id => document.getElementById(id);

// ─── Стратегии ────────────────────────────────────────────────────────────────

async function loadStrategies() {
    const sel = $('strategy-select');
    try {
        const strategies = await invoke('get_strategies');
        sel.innerHTML = '';

        if (!strategies || strategies.length === 0) {
            sel.innerHTML = '<option value="" disabled selected>Стратегии не найдены</option>';
            return;
        }

        strategies.forEach(name => {
            const opt = document.createElement('option');
            opt.value = name;
            opt.textContent = name;
            sel.appendChild(opt);
        });

        // general — по умолчанию, если есть
        const general = Array.from(sel.options).find(o => o.value === 'general');
        if (general) general.selected = true;

    } catch (err) {
        console.error('Ошибка загрузки стратегий:', err);
        sel.innerHTML = `<option value="" disabled selected>Ошибка: ${err}</option>`;
    }
}

// ─── Статус zapret ────────────────────────────────────────────────────────────

let serviceInstalled = false;

function updateStatusUI(status) {
    const sel = $('strategy-select');

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

        // Выбираем активную стратегию в dropdown
        if (status.strategy) {
            const match = Array.from(sel.options).find(o => o.value === status.strategy);
            if (match) match.selected = true;
        }
        sel.disabled = true;

    } else {
        $('header-status').textContent = 'Status: Disconnected';
        $('header-status').className =
            'text-[10px] text-error-dim font-bold uppercase tracking-[0.2em] opacity-80';

        $('hero-status').textContent = 'Disconnected';
        $('hero-status').className = 'text-error-dim';

        $('connect-btn-text').textContent = 'Establish Connection';
        $('connect-btn-icon').textContent = 'bolt';
        $('connect-btn').dataset.action = 'start';

        sel.disabled = false;
    }
}

function updateServiceUI(status) {
    serviceInstalled = !!status.installed;
    const label = $('service-status-label');
    const tempBtn = $('temporary-start-btn');
    const serviceBtn = $('service-enable-btn');

    if (status.installed) {
        const mode = status.running ? 'running' : 'installed';
        const strategy = status.strategy ? ` (${status.strategy})` : '';
        label.textContent = `${mode}${strategy}`;
        tempBtn.disabled = true;
        tempBtn.classList.add('opacity-50', 'cursor-not-allowed');
        serviceBtn.textContent = 'Disable service';
    } else {
        label.textContent = 'disabled';
        tempBtn.disabled = false;
        tempBtn.classList.remove('opacity-50', 'cursor-not-allowed');
        serviceBtn.textContent = 'Enable service';
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

async function pollService() {
    try {
        const status = await invoke('get_service_status');
        updateServiceUI(status);
    } catch (err) {
        console.error('Ошибка опроса service:', err);
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

function updateFiltersUI(filters) {
    // ── IPSet ──
    const ipsetOn = filters.ipset !== 'none';
    setToggle('ipset-toggle', ipsetOn);
    setCardActive('ipset-loaded', filters.ipset === 'loaded');
    setCardActive('ipset-any',    filters.ipset === 'any');

    // ── Game Filter ──
    const gameMode = filters.game_filter_raw ?? filters.game_filter;
    const gameOn = gameMode !== 'disabled';
    setToggle('game-toggle', gameOn);
    setCardActive('game-all', gameMode === 'all');
    setCardActive('game-tcp', gameMode === 'tcp');
    setCardActive('game-udp', gameMode === 'udp');
    $('game-filter-mode').textContent = `Mode: ${filters.game_filter_label ?? gameMode}`;
}

async function pollFilters() {
    try {
        const filters = await invoke('get_filters_status');
        updateFiltersUI(filters);
    } catch (err) {
        console.error('Ошибка опроса фильтров:', err);
    }
}

// ─── Кнопка Connect / Disconnect ─────────────────────────────────────────────

async function handleConnectClick() {
    const btn = $('connect-btn');
    const action = btn.dataset.action;
    btn.disabled = true;

    try {
        if (action === 'start') {
            if (serviceInstalled) {
                $('hero-status').textContent = 'Service mode is enabled';
                $('hero-status').className = 'text-secondary';
                return;
            }
            const strategy = $('strategy-select').value;
            if (!strategy) return;
            await invoke('start_zapret', { strategy });
        } else {
            await invoke('stop_zapret');
        }
        await pollStatus();
    } catch (err) {
        console.error('Ошибка действия:', err);
        $('hero-status').textContent = `Ошибка: ${err}`;
        $('hero-status').className = 'text-error-dim text-2xl';
        setTimeout(pollStatus, 3000);
    } finally {
        btn.disabled = false;
    }
}

async function handleServiceToggle() {
    const btn = $('service-enable-btn');
    btn.disabled = true;
    try {
        const strategy = $('strategy-select').value;
        if (!serviceInstalled) {
            if (!strategy) return;
            await invoke('start_zapret_service', { strategy });
            $('hero-status').textContent = 'Service mode enabled';
            $('hero-status').className = 'text-secondary';
        } else {
            await invoke('remove_zapret_service');
            $('hero-status').textContent = 'Service mode disabled';
            $('hero-status').className = 'text-error-dim';
        }
        await pollService();
        await pollStatus();
    } catch (err) {
        console.error('Ошибка переключения service mode:', err);
        $('hero-status').textContent = `Service error: ${err}`;
        $('hero-status').className = 'text-error-dim text-xl';
    } finally {
        btn.disabled = false;
    }
}

async function handleTemporaryStart() {
    if (serviceInstalled) return;
    const strategy = $('strategy-select').value;
    if (!strategy) return;
    try {
        await invoke('start_zapret', { strategy });
        await pollStatus();
    } catch (err) {
        console.error('Ошибка временного запуска:', err);
    }
}

// ─── Инициализация ────────────────────────────────────────────────────────────

window.addEventListener('DOMContentLoaded', async () => {
    await loadStrategies();

    // Сначала получаем статус — чтобы в dropdown сразу встала активная стратегия
    await pollStatus();
    await pollService();
    await pollFilters();

    // Поллинг каждые 2 секунды
    setInterval(async () => {
        await pollStatus();
        await pollService();
        await pollFilters();
    }, 2000);

    $('connect-btn').addEventListener('click', handleConnectClick);
    $('service-enable-btn').addEventListener('click', handleServiceToggle);
    $('temporary-start-btn').addEventListener('click', handleTemporaryStart);
});
