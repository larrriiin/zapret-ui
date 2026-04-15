use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use tauri::State;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

struct AppState {
    active_strategy: Mutex<Option<String>>,
}

#[derive(serde::Serialize)]
struct ZapretStatus {
    running: bool,
    strategy: Option<String>,
}

#[derive(serde::Serialize)]
struct FiltersStatus {
    /// "disabled" | "all" | "tcp" | "udp"
    game_filter: String,
    /// "none" | "any" | "loaded"
    ipset: String,
    /// "disabled" | "all" | "tcp" | "udp"
    game_filter_raw: String,
    /// Человекочитаемый статус как в service.bat
    game_filter_label: String,
}

#[derive(serde::Serialize)]
struct ServiceStatus {
    installed: bool,
    running: bool,
    strategy: Option<String>,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Ищет папку binaries/:
/// 1. Поднимается вверх от exe (продакшен и dev-режим)
/// 2. Проверяет текущую рабочую директорию
fn find_binaries_dir() -> PathBuf {
    // Обход дерева вверх от исполняемого файла
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..5 {
            if let Some(d) = &dir {
                let candidate = d.join("binaries");
                if candidate.exists() {
                    return candidate;
                }
                dir = d.parent().map(|p| p.to_path_buf());
            } else {
                break;
            }
        }
    }

    // Проверяем текущую рабочую директорию (работает в `tauri dev`)
    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join("binaries");
        if candidate.exists() {
            return candidate;
        }
    }

    PathBuf::from("binaries")
}

/// Проверяет, запущен ли winws.exe через tasklist.
fn is_winws_running() -> bool {
    let output = Command::new("tasklist")
        .args(["/fi", "IMAGENAME eq winws.exe", "/fo", "csv", "/nh"])
        .output();
    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .to_lowercase()
            .contains("winws.exe"),
        Err(_) => false,
    }
}

/// Читает активную стратегию из реестра Windows
/// (записывается при установке zapret как Windows-сервис).
fn get_strategy_from_registry() -> Option<String> {
    let out = Command::new("reg")
        .args([
            "query",
            "HKLM\\System\\CurrentControlSet\\Services\\zapret",
            "/v",
            "zapret-discord-youtube",
        ])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    // Строка: "    zapret-discord-youtube    REG_SZ    general (ALT)"
    for line in stdout.lines() {
        if line.contains("REG_SZ") {
            if let Some(pos) = line.find("REG_SZ") {
                let value = line[pos + "REG_SZ".len()..].trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

fn is_service_installed(name: &str) -> bool {
    Command::new("sc")
        .args(["query", name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn is_service_running(name: &str) -> bool {
    let output = Command::new("sc")
        .args(["query", name])
        .output();
    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout).contains("RUNNING"),
        Err(_) => false,
    }
}

fn run_hidden_cmd_with_args(args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new("cmd");
    cmd.args(args);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    cmd.spawn()
        .map_err(|e| format!("Не удалось запустить процесс: {}", e))?;
    Ok(())
}

fn get_service_sorted_bat_files(binaries_dir: &PathBuf) -> Result<Vec<String>, String> {
    let ps_script = format!(
        "Get-ChildItem -LiteralPath '{}' -Filter '*.bat' | Where-Object {{ $_.Name -notlike 'service*' }} | Sort-Object {{ [Regex]::Replace($_.Name, '(\\d+)', {{ $args[0].Value.PadLeft(8, '0') }}) }} | ForEach-Object {{ $_.Name }}",
        binaries_dir.to_string_lossy().replace('\'', "''")
    );

    let out = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_script])
        .output()
        .map_err(|e| format!("Не удалось получить список .bat через PowerShell: {}", e))?;

    if !out.status.success() {
        return Err("PowerShell не смог собрать список .bat файлов".to_string());
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let files: Vec<String> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|s| s.to_string())
        .collect();

    if files.is_empty() {
        return Err("Список стратегий для service.bat пуст".to_string());
    }

    Ok(files)
}

fn get_game_filter_mode_from_service_bat(binaries_dir: &PathBuf) -> Option<String> {
    let service_bat = binaries_dir.join("service.bat");
    if !service_bat.exists() {
        return None;
    }

    let cmdline = format!(
        "cd /d \"{}\" && call \"{}\" load_game_filter && echo __GF__!GameFilterStatus!__",
        binaries_dir.to_string_lossy(),
        service_bat.to_string_lossy()
    );

    let out = Command::new("cmd")
        .args(["/v:on", "/c", &cmdline])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let marker_start = "__GF__";
    let marker_end = "__";
    let start = stdout.find(marker_start)?;
    let rest = &stdout[start + marker_start.len()..];
    let end = rest.find(marker_end)?;
    let status = rest[..end].trim().to_lowercase();

    if status.contains("tcp and udp") {
        Some("all".to_string())
    } else if status.contains("(tcp)") {
        Some("tcp".to_string())
    } else if status.contains("(udp)") {
        Some("udp".to_string())
    } else if status.contains("disabled") {
        Some("disabled".to_string())
    } else {
        None
    }
}

// ─── Commands ─────────────────────────────────────────────────────────────────

/// Список стратегий — имена .bat файлов из binaries/ (без service.bat).
#[tauri::command]
fn get_strategies() -> Result<Vec<String>, String> {
    let dir = find_binaries_dir();
    let entries = std::fs::read_dir(&dir)
        .map_err(|e| format!("Не удалось прочитать binaries ({:?}): {}", dir, e))?;

    let mut list: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x.eq_ignore_ascii_case("bat"))
                .unwrap_or(false)
        })
        .filter_map(|e| {
            e.path()
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .filter(|name| name != "service")
        .collect();

    list.sort();
    Ok(list)
}

/// Текущий статус zapret: запущен ли и какая стратегия.
#[tauri::command]
fn get_zapret_status(state: State<'_, AppState>) -> ZapretStatus {
    let running = is_winws_running();
    let mut strategy_lock = state.active_strategy.lock().unwrap();

    if !running {
        *strategy_lock = None;
        return ZapretStatus { running: false, strategy: None };
    }

    if strategy_lock.is_some() {
        return ZapretStatus { running: true, strategy: strategy_lock.clone() };
    }

    // Пробуем определить из реестра (если запущен как Windows-сервис)
    let from_reg = get_strategy_from_registry();
    if from_reg.is_some() {
        *strategy_lock = from_reg.clone();
    }

    ZapretStatus { running: true, strategy: from_reg }
}

#[tauri::command]
fn get_service_status() -> ServiceStatus {
    let installed = is_service_installed("zapret");
    let running = if installed {
        is_service_running("zapret")
    } else {
        false
    };
    let strategy = if installed { get_strategy_from_registry() } else { None };

    ServiceStatus {
        installed,
        running,
        strategy,
    }
}

/// Состояние Game Filter и IPSet Filter по файлам конфигурации.
#[tauri::command]
fn get_filters_status() -> FiltersStatus {
    let dir = find_binaries_dir();

    // ── Game Filter: приоритетно читаем фактический статус из service.bat ──
    // Фолбэк — файл binaries/utils/game_filter.enabled.
    let game_filter = get_game_filter_mode_from_service_bat(&dir).unwrap_or_else(|| {
        let game_flag = dir.join("utils").join("game_filter.enabled");
        if !game_flag.exists() {
            "disabled".to_string()
        } else {
            let content = std::fs::read_to_string(&game_flag).unwrap_or_default();
            // Убираем BOM, пробелы, CRLF
            let mode = content
                .trim_start_matches('\u{FEFF}')
                .trim()
                .to_lowercase();
            match mode.as_str() {
                "tcp" => "tcp".to_string(),
                "udp" => "udp".to_string(),
                _ => "all".to_string(),
            }
        }
    });
    let game_filter_label = match game_filter.as_str() {
        "disabled" => "disabled".to_string(),
        "tcp" => "enabled (TCP)".to_string(),
        "udp" => "enabled (UDP)".to_string(),
        _ => "enabled (TCP and UDP)".to_string(),
    };

    // ── IPSet Filter: binaries/lists/ipset-all.txt ──
    let ipset_file = dir.join("lists").join("ipset-all.txt");
    let ipset = if !ipset_file.exists() {
        "any".to_string()
    } else {
        let content = std::fs::read_to_string(&ipset_file).unwrap_or_default();
        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.is_empty() {
            "any".to_string()
        } else if lines.iter().any(|l| l.trim() == "203.0.113.113/32") {
            "none".to_string()
        } else {
            "loaded".to_string()
        }
    };

    FiltersStatus {
        game_filter_raw: game_filter.clone(),
        game_filter,
        game_filter_label,
        ipset,
    }
}

/// Запускает стратегию по имени .bat файла.
#[tauri::command]
fn start_zapret(strategy: String, state: State<'_, AppState>) -> Result<String, String> {
    // Убиваем текущий процесс (без прав сервис остановится через bat)
    let _ = Command::new("taskkill").args(["/f", "/im", "winws.exe"]).output();

    let bat_path = find_binaries_dir().join(format!("{}.bat", strategy));
    if !bat_path.exists() {
        return Err(format!("Файл стратегии не найден: {}.bat", strategy));
    }

    let bat_str = bat_path
        .to_str()
        .ok_or("Невалидный путь к bat-файлу")?
        .to_string();

    run_hidden_cmd_with_args(&["/c", &bat_str])
        .map_err(|e| format!("Не удалось запустить стратегию: {}", e))?;

    *state.active_strategy.lock().unwrap() = Some(strategy);
    Ok("Connected".into())
}

#[tauri::command]
fn start_zapret_service(strategy: String, state: State<'_, AppState>) -> Result<String, String> {
    let binaries_dir = find_binaries_dir();
    let service_bat = binaries_dir.join("service.bat");
    if !service_bat.exists() {
        return Err("service.bat не найден в папке binaries".to_string());
    }

    // Индекс стратегии как в service.bat (натуральная сортировка через PowerShell).
    let files = get_service_sorted_bat_files(&binaries_dir)?;

    let target = format!("{}.bat", strategy);
    let index = files
        .iter()
        .position(|f| f.eq_ignore_ascii_case(&target))
        .map(|i| i + 1)
        .ok_or_else(|| format!("Стратегия не найдена для установки в сервис: {}", strategy))?;

    // Автоматизируем меню service.bat:
    // 1 -> Install Service, индекс стратегии, Enter для pause, 0 -> Exit
    let automation = format!("echo 1&echo {}&echo.&echo 0", index);
    let cmdline = format!(
        "cd /d \"{}\" && ({}) | \"{}\" admin",
        binaries_dir.to_string_lossy(),
        automation,
        service_bat.to_string_lossy()
    );

    let out = Command::new("cmd")
        .args(["/c", &cmdline])
        .output()
        .map_err(|e| format!("Не удалось запустить установку сервиса: {}", e))?;

    if !out.status.success() || !is_service_installed("zapret") {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Не удалось установить сервис. {}", err.trim()));
    }

    *state.active_strategy.lock().unwrap() = Some(strategy);
    Ok("Service mode enabled".into())
}

#[tauri::command]
fn remove_zapret_service(state: State<'_, AppState>) -> Result<String, String> {
    let binaries_dir = find_binaries_dir();
    let service_bat = binaries_dir.join("service.bat");
    if !service_bat.exists() {
        return Err("service.bat не найден в папке binaries".to_string());
    }

    // 2 -> Remove Services, Enter для pause, 0 -> Exit
    let cmdline = format!(
        "cd /d \"{}\" && (echo 2&echo.&echo 0) | \"{}\" admin",
        binaries_dir.to_string_lossy(),
        service_bat.to_string_lossy()
    );
    let out = Command::new("cmd")
        .args(["/c", &cmdline])
        .output()
        .map_err(|e| format!("Не удалось удалить сервис: {}", e))?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Ошибка удаления сервиса. {}", err.trim()));
    }
    if is_service_installed("zapret") {
        return Err("Сервис zapret всё ещё установлен после попытки удаления".to_string());
    }

    *state.active_strategy.lock().unwrap() = None;
    Ok("Service mode disabled".into())
}

/// Полностью останавливает zapret.
/// Требует прав администратора — запрашивает их через PowerShell -Verb RunAs.
#[tauri::command]
fn stop_zapret(state: State<'_, AppState>) {
    // Пишем bat-файл со всеми командами остановки во временную папку
    let bat_path = std::env::temp_dir().join("zapret_stop.bat");

    let bat_content = concat!(
        "@echo off\r\n",
        // Останавливаем и удаляем сервис zapret
        "net stop zapret 2>nul\r\n",
        "sc delete zapret 2>nul\r\n",
        // Убиваем процесс winws.exe
        "taskkill /F /IM winws.exe 2>nul\r\n",
        // Останавливаем и удаляем WinDivert
        "net stop WinDivert 2>nul\r\n",
        "sc delete WinDivert 2>nul\r\n",
        "net stop WinDivert14 2>nul\r\n",
        "sc delete WinDivert14 2>nul\r\n"
    );

    if std::fs::write(&bat_path, bat_content).is_ok() {
        // Запускаем bat с правами администратора через PowerShell RunAs.
        // %TEMP% в аргументе cmd.exe раскрывается самим cmd, избегая проблем с пробелами в пути.
        let _ = Command::new("powershell")
            .args([
                "-NoProfile",
                "-WindowStyle", "Hidden",
                "-Command",
                "Start-Process cmd.exe -ArgumentList '/c %TEMP%\\zapret_stop.bat' -Verb RunAs -Wait -WindowStyle Hidden",
            ])
            .output();
    }

    *state.active_strategy.lock().unwrap() = None;
}

// ─── Entry point ──────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            active_strategy: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            get_strategies,
            get_zapret_status,
            get_filters_status,
            start_zapret,
            start_zapret_service,
            stop_zapret,
            remove_zapret_service,
            get_service_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
