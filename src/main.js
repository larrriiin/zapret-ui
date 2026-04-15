const { invoke } = window.__TAURI__.core;

// ─── DOM-элементы ────────────────────────────────────────────────────────────

const headerStatus  = () => document.getElementById('header-status');
const heroStatus    = () => document.getElementById('hero-status');
const connectBtn    = () => document.getElementById('connect-btn');
const connectText   = () => document.getElementById('connect-btn-text');
const connectIcon   = () => document.getElementById('connect-btn-icon');
const strategySelect = () => document.getElementById('strategy-select');

// ─── Загрузка списка стратегий ────────────────────────────────────────────────

async function loadStrategies() {
    const sel = strategySelect();
    try {
        const strategies = await invoke('get_strategies');

        sel.innerHTML = '';

        if (!strategies || strategies.length === 0) {
            sel.innerHTML = '<option value="" disabled selected>Стратегии не найдены</option>';
            return;
        }

        // «general» — первым, остальные идут по алфавиту (уже отсортированы Rust-ом)
        strategies.forEach(name => {
            const opt = document.createElement('option');
            opt.value = name;
            opt.textContent = name;
            sel.appendChild(opt);
        });

        // Выбираем «general» по умолчанию если он есть
        const generalOpt = Array.from(sel.options).find(o => o.value === 'general');
        if (generalOpt) generalOpt.selected = true;

    } catch (err) {
        console.error('Ошибка загрузки стратегий:', err);
        sel.innerHTML = `<option value="" disabled selected>Ошибка: ${err}</option>`;
    }
}

// ─── Обновление UI по статусу ─────────────────────────────────────────────────

function updateUI(status) {
    const btn  = connectBtn();
    const sel  = strategySelect();

    if (status.running) {
        // ── Подключено ──
        const label = status.strategy ?? 'Connected';

        headerStatus().textContent = `Status: ${label}`;
        headerStatus().className =
            'text-[10px] font-bold uppercase tracking-[0.2em] opacity-80 text-secondary';

        heroStatus().textContent = 'Connected';
        heroStatus().className = 'text-secondary';

        connectText().textContent = 'Disconnect';
        connectIcon().textContent = 'power_settings_new';
        btn.dataset.action = 'stop';

        // Синхронизируем dropdown с активной стратегией
        if (status.strategy) {
            const match = Array.from(sel.options).find(o => o.value === status.strategy);
            if (match) match.selected = true;
        }
        sel.disabled = true;

    } else {
        // ── Отключено ──
        headerStatus().textContent = 'Status: Disconnected';
        headerStatus().className =
            'text-[10px] text-error-dim font-bold uppercase tracking-[0.2em] opacity-80';

        heroStatus().textContent = 'Disconnected';
        heroStatus().className = 'text-error-dim';

        connectText().textContent = 'Establish Connection';
        connectIcon().textContent = 'bolt';
        btn.dataset.action = 'start';

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
    const btn    = connectBtn();
    const action = btn.dataset.action;

    btn.disabled = true;

    try {
        if (action === 'start') {
            const strategy = strategySelect().value;
            if (!strategy) {
                btn.disabled = false;
                return;
            }
            await invoke('start_zapret', { strategy });
        } else {
            await invoke('stop_zapret');
        }
        // Сразу обновляем UI после действия
        await pollStatus();
    } catch (err) {
        console.error('Ошибка действия:', err);
        // Показываем ошибку в hero-заголовке на 3 секунды
        heroStatus().textContent = `Ошибка: ${err}`;
        heroStatus().className = 'text-error-dim text-2xl';
        setTimeout(pollStatus, 3000);
    } finally {
        btn.disabled = false;
    }
}

// ─── Инициализация ────────────────────────────────────────────────────────────

window.addEventListener('DOMContentLoaded', async () => {
    await loadStrategies();
    await pollStatus();

    // Поллинг каждые 2 секунды
    setInterval(pollStatus, 2000);

    connectBtn().addEventListener('click', handleConnectClick);
});
