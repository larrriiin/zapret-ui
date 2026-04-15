// invoke получаем лениво, чтобы не было гонки с инициализацией Tauri
function invoke(cmd, args) {
    return window.__TAURI__.core.invoke(cmd, args);
}

// ─── DOM-элементы ────────────────────────────────────────────────────────────

const $ = id => document.getElementById(id);

// ─── Загрузка списка стратегий ────────────────────────────────────────────────

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

        // Выбираем «general» по умолчанию если есть
        const general = Array.from(sel.options).find(o => o.value === 'general');
        if (general) general.selected = true;

    } catch (err) {
        console.error('Ошибка загрузки стратегий:', err);
        sel.innerHTML = `<option value="" disabled selected>Ошибка: ${err}</option>`;
    }
}

// ─── Обновление UI по статусу ─────────────────────────────────────────────────

function updateUI(status) {
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

// ─── Поллинг статуса ──────────────────────────────────────────────────────────

async function pollStatus() {
    try {
        const status = await invoke('get_zapret_status');
        updateUI(status);
    } catch (err) {
        console.error('Ошибка опроса статуса:', err);
    }
}

// ─── Кнопка Connect / Disconnect ─────────────────────────────────────────────

async function handleConnectClick() {
    const btn = $('connect-btn');
    const action = btn.dataset.action;

    btn.disabled = true;

    try {
        if (action === 'start') {
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

// ─── Инициализация ────────────────────────────────────────────────────────────

window.addEventListener('DOMContentLoaded', async () => {
    await loadStrategies();
    await pollStatus();
    setInterval(pollStatus, 2000);
    $('connect-btn').addEventListener('click', handleConnectClick);
});
