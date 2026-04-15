use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use tauri::State;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x08000000;

struct AppState {
    active_strategy: Mutex<Option<String>>,
}

#[derive(serde::Serialize)]
struct ZapretStatus {
    running: bool,
    strategy: Option<String>,
    mode: Option<String>,
}

#[derive(serde::Serialize)]
struct FiltersStatus {
    /// "disabled" | "all" | "tcp" | "udp"
    game_filter: String,
    /// "none" | "any" | "loaded"
    ipset: String,
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

fn parse_bat_args(strategy: &str) -> Result<String, String> {
    let dir = find_binaries_dir();
    let bat_path = dir.join(format!("{}.bat", strategy));
    let content = std::fs::read_to_string(&bat_path)
        .map_err(|e| format!("Не удалось прочитать {}.bat: {}", strategy, e))?;

    // Читаем текущие значения фильтров для подстановки
    let filters = get_filters_status();
    let game_filter_mode = filters.game_filter;

    let (gf, gftcp, gfudp) = match game_filter_mode.as_str() {
        "all" => ("1024-65535", "1024-65535", "1024-65535"),
        "tcp" => ("1024-65535", "1024-65535", "12"),
        "udp" => ("1024-65535", "12", "1024-65535"),
        _ => ("12", "12", "12"),
    };

    let bin_path = dir.join("bin").to_str().unwrap_or("").to_string() + "\\";
    let lists_path = dir.join("lists").to_str().unwrap_or("").to_string() + "\\";
    let root_path = dir.to_str().unwrap_or("").to_string() + "\\";

    // Ищем строку с запуском winws.exe
    for line in content.lines() {
        let line_lower = line.to_lowercase();
        if line_lower.contains("winws.exe") {
            let mut args = String::new();
            if let Some(pos) = line_lower.find("winws.exe\"") {
                args = line[pos + "winws.exe\"".len()..].to_string();
            } else if let Some(pos) = line_lower.find("winws.exe ") {
                args = line[pos + "winws.exe ".len()..].to_string();
            }

            // Убираем символы продолжения строки `^`
            args = args.replace("^", "");

            // Подстановка переменных (эмуляция service.bat)
            args = args.replace("%GameFilter%", gf);
            args = args.replace("%GameFilterTCP%", gftcp);
            args = args.replace("%GameFilterUDP%", gfudp);
            args = args.replace("%BIN%", &bin_path);
            args = args.replace("%LISTS%", &lists_path);

            // Замена @ на абсолютный путь к корню binaries
            let mut final_args = String::new();
            for word in args.split_whitespace() {
                let mut w = word.to_string();
                if w.starts_with("\"@") {
                    w = format!("\"{}{}", root_path, &w[2..]);
                }
                // Экранируем кавычки для SC CREATE
                w = w.replace("\"", "\\\"");
                final_args.push_str(&w);
                final_args.push(' ');
            }

            return Ok(final_args.trim().to_string());
        }
    }

    Ok("".to_string())
}

/// Проверяет, запущен ли winws.exe через tasklist.
fn is_zapret_service_running() -> bool {
    let output = Command::new("sc")
        .args(["query", "zapret"])
        .output();
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_lowercase();
            stdout.contains("running") || stdout.contains("start_pending")
        },
        Err(_) => false,
    }
}

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
    let mut running = is_winws_running();
    let is_service = is_zapret_service_running();
    if is_service {
        running = true;
    }

    let mut strategy_lock = state.active_strategy.lock().unwrap();

    if !running {
        *strategy_lock = None;
        return ZapretStatus { running: false, strategy: None, mode: None };
    }

    let mode = if is_service { Some("service".to_string()) } else { Some("temporary".to_string()) };

    if strategy_lock.is_some() {
        return ZapretStatus { running: true, strategy: strategy_lock.clone(), mode };
    }

    // Пробуем определить из реестра (если запущен как Windows-сервис)
    let from_reg = get_strategy_from_registry();
    if from_reg.is_some() {
        *strategy_lock = from_reg.clone();
    }

    ZapretStatus { running: true, strategy: from_reg, mode }
}

/// Состояние Game Filter и IPSet Filter по файлам конфигурации.
#[tauri::command]
fn get_filters_status() -> FiltersStatus {
    let dir = find_binaries_dir();

    // ── Game Filter: binaries/utils/game_filter.enabled ──
    let game_flag = dir.join("utils").join("game_filter.enabled");
    let game_filter = if !game_flag.exists() {
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

    FiltersStatus { game_filter, ipset }
}

#[tauri::command]
fn set_game_filter(mode: String) -> Result<(), String> {
    let dir = find_binaries_dir();
    let game_flag = dir.join("utils").join("game_filter.enabled");

    if mode == "disabled" {
        if game_flag.exists() {
            let _ = std::fs::remove_file(game_flag);
        }
    } else {
        std::fs::write(&game_flag, mode).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Запускает стратегию по имени .bat файла.
#[tauri::command]
fn start_zapret(strategy: String, mode: String, state: State<'_, AppState>) -> Result<String, String> {
    // Убиваем текущий процесс
    let _ = Command::new("taskkill").args(["/f", "/im", "winws.exe"]).output();

    let dir = find_binaries_dir();
    let bat_path = dir.join(format!("{}.bat", strategy));
    if !bat_path.exists() {
        return Err(format!("Файл стратегии не найден: {}.bat", strategy));
    }

    if mode == "service" {
        let args = parse_bat_args(&strategy)?;
        let bin_path = dir.join("bin").join("winws.exe");
        let bin_str = bin_path.to_str().unwrap_or_default();

        let bat_content = format!(
            "@echo off\r\n\
             net stop zapret 2>nul\r\n\
             sc delete zapret 2>nul\r\n\
             sc create zapret binPath= \"\\\"{}\\\" {}\" DisplayName= \"zapret\" start= auto\r\n\
             sc description zapret \"Zapret DPI bypass software\"\r\n\
             sc start zapret\r\n\
             reg add \"HKLM\\System\\CurrentControlSet\\Services\\zapret\" /v zapret-discord-youtube /t REG_SZ /d \"{}\" /f\r\n",
             bin_str, args, strategy
        );

        let bat_path = std::env::temp_dir().join("zapret_start.bat");
        if std::fs::write(&bat_path, bat_content).is_ok() {
            let mut cmd = Command::new("powershell");
            cmd.args([
                "-NoProfile",
                "-WindowStyle", "Hidden",
                "-Command",
                "Start-Process cmd.exe -ArgumentList '/c %TEMP%\\zapret_start.bat' -Verb RunAs -Wait -WindowStyle Hidden",
            ]);
            #[cfg(windows)]
            cmd.creation_flags(CREATE_NO_WINDOW);
            let _ = cmd.output();
        } else {
            return Err("Не удалось создать bat-файл для запуска сервиса".to_string());
        }
    } else {
        let bat_str = bat_path
            .to_str()
            .ok_or("Невалидный путь к bat-файлу")?
            .to_string();

        let mut cmd = Command::new("cmd");
        cmd.args(["/c", &bat_str]);
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);

        cmd.spawn()
            .map_err(|e| format!("Не удалось запустить стратегию: {}", e))?;
    }

    *state.active_strategy.lock().unwrap() = Some(strategy);
    Ok("Connected".into())
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
            set_game_filter,
            start_zapret,
            stop_zapret,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
