use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::Mutex;
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::Emitter;
use tauri::Manager;
use tauri::State;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use std::sync::atomic::{AtomicBool, Ordering};
use tauri_plugin_notification::NotificationExt;

const CREATE_NO_WINDOW: u32 = 0x08000000;
const GITHUB_VERSION_URL: &str =
    "https://raw.githubusercontent.com/Flowseal/zapret-discord-youtube/main/.service/version.txt";

struct AppState {
    active_strategy: Mutex<Option<String>>,
    test_process_pid: Mutex<Option<u32>>,
    status_item: Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>>,
    strategy_item: Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>>,
    toggle_item: Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>>,
    quit_item: Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>>,
    show_item: Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>>,
    strategies_submenu: Mutex<Option<tauri::menu::Submenu<tauri::Wry>>>,
    tray_handle: Mutex<Option<tauri::tray::TrayIcon<tauri::Wry>>>,
    notification_shown: AtomicBool,
    last_strategy: Mutex<Option<String>>,
    translations: Mutex<Option<TrayTranslations>>,
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
struct TrayTranslations {
    exit: String,
    show: String,
    status_prefix: String,
    strategy_prefix: String,
    toggle_on: String,
    toggle_off: String,
    change_strategy: String,
    minimized_title: String,
    minimized_body: String,
    status_on: String,
    status_off: String,
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

fn find_binaries_dir() -> PathBuf {
    // 1. Direct sibling of the exe (production after first download)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("binaries");
            if candidate.exists() {
                return candidate;
            }
        }
    }

    // 2. Climb up from exe (dev mode: exe is deep inside src-tauri/target/debug)
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

    // 3. CWD fallback (tauri dev)
    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join("binaries");
        if candidate.exists() {
            return candidate;
        }
    }

    // 4. Default: next to exe (will be created on first download)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return parent.join("binaries");
        }
    }

    PathBuf::from("binaries")
}

fn is_admin() -> bool {
    // net session — самый быстрый и надежный способ проверки прав администратора на Windows
    Command::new("net")
        .arg("session")
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn elevate_if_needed() {
    if !is_admin() {
        if let Ok(exe) = std::env::current_exe() {
            let args: Vec<String> = std::env::args().skip(1).collect();

            let ps_args = if args.is_empty() {
                String::new()
            } else {
                let formatted = args
                    .iter()
                    .map(|s| format!("'{}'", s.replace("'", "''")))
                    .collect::<Vec<String>>()
                    .join(",");
                format!("-ArgumentList @({})", formatted)
            };

            let ps_command = format!(
                "Start-Process -FilePath '{}' {} -Verb RunAs",
                exe.to_string_lossy().replace("'", "''"),
                ps_args
            );

            let _ = Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-WindowStyle",
                    "Hidden",
                    "-Command",
                    &ps_command,
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();

            std::process::exit(0);
        }
    }
}

fn get_local_version() -> String {
    let dir = find_binaries_dir();
    let service_bat = dir.join("service.bat");
    
    if !service_bat.exists() {
        return format!("Err: Not Found at {:?}", service_bat);
    }

    match std::fs::read_to_string(&service_bat) {
        Ok(content) => {
            for line in content.lines() {
                let lowercase = line.to_lowercase();
                if lowercase.contains("local_version=") {
                    let parts: Vec<&str> = line.splitn(2, '=').collect();
                    if parts.len() > 1 {
                        let version = parts[1].trim().trim_matches('"');
                        if !version.is_empty() {
                            return version.to_string();
                        }
                    }
                }
            }
            "Err: No Version String Found".to_string()
        }
        Err(e) => format!("Err: Read Failed ({})", e),
    }
}

#[tauri::command]
fn get_local_version_cmd() -> String {
    get_local_version()
}

#[tauri::command]
async fn get_remote_core_version() -> Result<String, String> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://raw.githubusercontent.com/Flowseal/zapret-discord-youtube/main/.service/version.txt")
        .send()
        .await
        .map_err(|e: reqwest::Error| e.to_string())?;
    
    let text = response.text().await.map_err(|e: reqwest::Error| e.to_string())?;
    Ok(text.trim().to_string())
}

fn get_ui_version() -> String {
    // APP_VERSION is dynamically set by build.rs from tauri.conf.json
    env!("APP_VERSION").to_string()
}

#[tauri::command]
fn get_ui_version_cmd() -> String {
    get_ui_version()
}

#[tauri::command]
fn ensure_binaries_present() -> bool {
    let bin_dir = find_binaries_dir();
    // In production, binaries is a folder inside resources.
    // In dev, it's next to src-tauri.
    // find_binaries_dir already handles this.
    bin_dir.exists() && bin_dir.join("service.bat").exists()
}

fn parse_bat_args(strategy: &str) -> Result<String, String> {
    let dir = find_binaries_dir();
    let bat_path = dir.join(format!("{}.bat", strategy));
    let content = std::fs::read_to_string(&bat_path)
        .map_err(|e| format!("Не удалось прочитать {}.bat: {}", strategy, e))?;

    // Читаем текущие значения фильтров для подстановки
    let filters = get_filters_status();
    let game_filter_mode = filters.game_filter;

    // Для disabled используем "12" (порт не используется)
    let (gf, gftcp, gfudp) = match game_filter_mode.as_str() {
        "all" => ("1024-65535", "1024-65535", "1024-65535"),
        "tcp" => ("1024-65535", "1024-65535", "12"),
        "udp" => ("1024-65535", "12", "1024-65535"),
        _ => ("12", "12", "12"),
    };

    let bin_path = dir.join("bin").to_str().unwrap_or("").to_string() + "\\";
    let lists_path = dir.join("lists").to_str().unwrap_or("").to_string() + "\\";
    let root_path = dir.to_str().unwrap_or("").to_string() + "\\";

    // Ищем строку с запуском winws.exe и собираем все строки продолжения (^)
    let lines: Vec<&str> = content.lines().collect();
    let mut found_idx: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if line.to_lowercase().contains("winws.exe") {
            found_idx = Some(i);
            break;
        }
    }

    let found_idx =
        found_idx.ok_or_else(|| format!("Не найдена строка с winws.exe в {}.bat", strategy))?;

    // Собираем полную команду: первая строка + все строки-продолжения (^)
    let mut full_command = String::new();
    for i in found_idx..lines.len() {
        let line = lines[i].trim();
        if line.ends_with('^') {
            full_command.push_str(&line[..line.len() - 1]);
            full_command.push(' ');
        } else {
            full_command.push_str(line);
            break;
        }
    }

    eprintln!("[DEBUG] Full command for '{}': {}", strategy, full_command);

    // Извлекаем аргументы после winws.exe
    let cmd_lower = full_command.to_lowercase();
    let mut args = String::new();
    if let Some(pos) = cmd_lower.find("winws.exe\"") {
        args = full_command[pos + "winws.exe\"".len()..].to_string();
    } else if let Some(pos) = cmd_lower.find("winws.exe ") {
        args = full_command[pos + "winws.exe ".len()..].to_string();
    }

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

    let result = final_args.trim().to_string();
    eprintln!(
        "[DEBUG] Parsed args for strategy '{}': {}",
        strategy, result
    );
    Ok(result)
}

/// Проверяет, запущен ли winws.exe через tasklist.
fn is_zapret_service_running() -> bool {
    let output = Command::new("sc")
        .args(["query", "zapret"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_lowercase();
            stdout.contains("running") || stdout.contains("start_pending")
        }
        Err(_) => false,
    }
}

fn is_winws_running() -> bool {
    let output = Command::new("tasklist")
        .args(["/fi", "IMAGENAME eq winws.exe", "/fo", "csv", "/nh"])
        .creation_flags(CREATE_NO_WINDOW)
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
        .creation_flags(CREATE_NO_WINDOW)
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

#[tauri::command]
fn check_status_full() -> Result<String, String> {
    let mut output = String::new();

    // 1. Check Strategy
    let reg_out = Command::new("reg")
        .args([
            "query",
            "HKLM\\System\\CurrentControlSet\\Services\\zapret",
            "/v",
            "zapret-discord-youtube",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    if let Ok(out) = reg_out {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            if let Some(pos) = line.find("REG_SZ") {
                let strategy = line[pos + "REG_SZ".len()..].trim();
                if !strategy.is_empty() {
                    output.push_str(&format!(
                        "Service strategy installed from \"{}\"\n",
                        strategy
                    ));
                }
                break;
            }
        }
    }

    // 2. Check zapret service
    let zapret_svc = Command::new("sc")
        .args(["query", "zapret"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    if let Ok(out) = zapret_svc {
        let stdout = String::from_utf8_lossy(&out.stdout);
        if stdout.contains("RUNNING") {
            output.push_str("\"zapret\" service is RUNNING.\n");
        } else if stdout.contains("STOPPED") {
            output.push_str("\"zapret\" service is STOPPED.\n");
        } else if stdout.contains("FAILED 1060") || stdout.contains("1060") {
            // 1060 means service does not exist
        } else {
            // Might be start_pending or other
            output.push_str(&format!("\"zapret\" service state is UNKNOWN.\n"));
        }
    }

    // 3. Check WinDivert service
    let windivert_svc = Command::new("sc")
        .args(["query", "WinDivert"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    if let Ok(out) = windivert_svc {
        let stdout = String::from_utf8_lossy(&out.stdout);
        if stdout.contains("RUNNING") {
            output.push_str("\"WinDivert\" service is RUNNING.\n");
        } else if stdout.contains("STOPPED") {
            output.push_str("\"WinDivert\" service is STOPPED.\n");
        }
    }

    // 4. Check bypass (winws.exe)
    output.push_str("\n");
    let task = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq winws.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    if let Ok(out) = task {
        let stdout = String::from_utf8_lossy(&out.stdout).to_lowercase();
        if stdout.contains("winws.exe") {
            output.push_str("Bypass (winws.exe) is RUNNING.\n");
        } else {
            output.push_str("Bypass (winws.exe) is NOT running.\n");
        }
    }

    let trimmed = output.trim().to_string();
    if trimmed.is_empty() {
        Ok("Zapret service is not installed.".to_string())
    } else {
        Ok(trimmed)
    }
}

/// Список стратегий — имена .bat файлов из binaries/ (без service.bat).
#[tauri::command]
fn get_strategies() -> Result<Vec<String>, String> {
    let dir = find_binaries_dir();
    let entries = std::fs::read_dir(&dir)
        .map_err(|e| format!("Failed to read binaries ({:?}): {}", dir, e))?;

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

    // Natural sort (Windows-style) so ALT2 comes before ALT11
    list.sort_by(|a, b| natural_sort_compare(a, b));
    Ok(list)
}

/// Compare strings using natural sort (numbers compared numerically)
fn natural_sort_compare(a: &str, b: &str) -> std::cmp::Ordering {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();

    let mut i = 0;
    let mut j = 0;

    while i < a_chars.len() && j < b_chars.len() {
        let ca = a_chars[i];
        let cb = b_chars[j];

        // If both are digits, compare the full numbers
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            // Extract full number from a
            let mut num_a = 0u32;
            let start_i = i;
            while i < a_chars.len() && a_chars[i].is_ascii_digit() {
                num_a = num_a * 10 + (a_chars[i] as u32 - '0' as u32);
                i += 1;
            }

            // Extract full number from b
            let mut num_b = 0u32;
            let start_j = j;
            while j < b_chars.len() && b_chars[j].is_ascii_digit() {
                num_b = num_b * 10 + (b_chars[j] as u32 - '0' as u32);
                j += 1;
            }

            // Compare numbers
            if num_a != num_b {
                return num_a.cmp(&num_b);
            }

            // Numbers are equal but different lengths (e.g., "01" vs "1")
            let len_a = i - start_i;
            let len_b = j - start_j;
            if len_a != len_b {
                return len_a.cmp(&len_b);
            }
        } else {
            // Compare characters case-insensitively
            let cmp = ca.to_ascii_lowercase().cmp(&cb.to_ascii_lowercase());
            if cmp != std::cmp::Ordering::Equal {
                return cmp;
            }
            i += 1;
            j += 1;
        }
    }

    // If one string is exhausted, the shorter one comes first
    a_chars.len().cmp(&b_chars.len())
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
        return ZapretStatus {
            running: false,
            strategy: None,
            mode: None,
        };
    }

    let mode = if is_service {
        Some("service".to_string())
    } else {
        Some("temporary".to_string())
    };

    if strategy_lock.is_some() {
        return ZapretStatus {
            running: true,
            strategy: strategy_lock.clone(),
            mode,
        };
    }

    // Пробуем определить из реестра (если запущен как Windows-сервис)
    let from_reg = get_strategy_from_registry();
    if from_reg.is_some() {
        *strategy_lock = from_reg.clone();
    }

    ZapretStatus {
        running: true,
        strategy: from_reg,
        mode,
    }
}

/// Состояние Game Filter и IPSet Filter по файлам конфигурации.
#[tauri::command]
fn get_filters_status() -> FiltersStatus {
    let dir = find_binaries_dir();

    // ── Game Filter: binaries/utils/game_filter.enabled ──
    // Консольная версия: отсутствие файла = disabled
    let game_flag = dir.join("utils").join("game_filter.enabled");
    let game_filter = if !game_flag.exists() {
        "disabled".to_string()
    } else {
        let content = std::fs::read_to_string(&game_flag).unwrap_or_default();
        // Убираем BOM, пробелы, CRLF
        let mode = content.trim_start_matches('\u{FEFF}').trim().to_lowercase();
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
        // Удаляем файл для совместимости с консольной версией
        // Консольная версия считает отсутствие файла = disabled
        if game_flag.exists() {
            let _ = std::fs::remove_file(&game_flag);
        }
    } else {
        std::fs::write(&game_flag, mode).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn set_ipset_filter(mode: String) -> Result<(), String> {
    let dir = find_binaries_dir();
    let ipset_file = dir.join("lists").join("ipset-all.txt");
    let backup_file = dir.join("lists").join("ipset-all.txt.backup");

    match mode.as_str() {
        "none" => {
            // Записываем dummy IP для состояния none
            std::fs::write(&ipset_file, "203.0.113.113/32\n").map_err(|e| e.to_string())?;
        }
        "any" => {
            // Перед тем как сделать пустой файл, сохраняем бэкап если есть реальные данные
            // (не пустой и не содержащий dummy IP)
            if ipset_file.exists() && !backup_file.exists() {
                let content = std::fs::read_to_string(&ipset_file).unwrap_or_default();
                let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
                if !lines.is_empty() && !lines.iter().any(|l| l.trim() == "203.0.113.113/32") {
                    std::fs::copy(&ipset_file, &backup_file).map_err(|e| e.to_string())?;
                }
            }
            // Создаем пустой файл
            std::fs::write(&ipset_file, "").map_err(|e| e.to_string())?;
        }
        "loaded" => {
            // Восстанавливаем из бэкапа если он есть и содержит реальные данные
            if backup_file.exists() {
                let backup_content = std::fs::read_to_string(&backup_file).unwrap_or_default();
                let backup_lines: Vec<&str> = backup_content
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .collect();
                // Проверяем что бэкап не содержит dummy IP
                if !backup_lines.is_empty()
                    && !backup_lines.iter().any(|l| l.trim() == "203.0.113.113/32")
                {
                    std::fs::copy(&backup_file, &ipset_file).map_err(|e| e.to_string())?;
                } else {
                    // Бэкап поврежден (содержит none), создаем дефолтный
                    let default_ips = "185.65.148.0/22\n192.229.128.0/17\n";
                    std::fs::write(&ipset_file, default_ips).map_err(|e| e.to_string())?;
                }
            } else {
                // Если бэкапа нет, создаем дефолтный список
                let default_ips = "185.65.148.0/22\n192.229.128.0/17\n";
                std::fs::write(&ipset_file, default_ips).map_err(|e| e.to_string())?;
            }
        }
        _ => return Err(format!("Invalid IPSet mode: {}", mode)),
    }

    Ok(())
}

/// Запускает стратегию по имени .bat файла.
#[tauri::command]
fn start_zapret(
    strategy: String,
    mode: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    // Убиваем текущий процесс
    let _ = Command::new("taskkill")
        .args(["/f", "/im", "winws.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    let dir = find_binaries_dir();
    let bat_path = dir.join(format!("{}.bat", strategy));
    if !bat_path.exists() {
        return Err(format!("Файл стратегии не найден: {}.bat", strategy));
    }

    // Убеждаемся, что пользовательские списки существуют, иначе winws не запустится
    let lists_dir = dir.join("lists");
    if !lists_dir.exists() {
        let _ = std::fs::create_dir_all(&lists_dir);
    }
    let ipset_user = lists_dir.join("ipset-exclude-user.txt");
    if !ipset_user.exists() {
        let _ = std::fs::write(&ipset_user, "203.0.113.113/32\r\n");
    }
    let list_general_user = lists_dir.join("list-general-user.txt");
    if !list_general_user.exists() {
        let _ = std::fs::write(&list_general_user, "domain.example.abc\r\n");
    }
    let list_exclude_user = lists_dir.join("list-exclude-user.txt");
    if !list_exclude_user.exists() {
        let _ = std::fs::write(&list_exclude_user, "domain.example.abc\r\n");
    }

    if mode == "service" {
        let args = parse_bat_args(&strategy)?;
        let bin_path = dir.join("bin").join("winws.exe");
        let bin_str = bin_path.to_str().unwrap_or_default();

        // Проверяем что аргументы не пустые
        if args.is_empty() {
            return Err("Не удалось распарсить аргументы из bat файла".to_string());
        }

        let bin_dir_path = std::path::Path::new(&bin_str)
            .parent()
            .unwrap_or(std::path::Path::new(""));
        let bin_name = std::path::Path::new(&bin_str)
            .file_name()
            .unwrap_or_default()
            .to_str()
            .unwrap_or("winws.exe");

        let bat_content = format!(
            "@echo off\r\n\
             echo Initializing WinDivert driver (elevated probe)...\r\n\
             cd /d \"{}\"\r\n\
             \"{}\" --version >nul 2>&1\r\n\
             echo Stopping existing service...\r\n\
             net stop zapret 2>nul\r\n\
             sc delete zapret 2>nul\r\n\
             echo Creating service...\r\n\
             sc create zapret binPath= \"\\\"{}\\\" {}\" DisplayName= \"zapret\" start= auto\r\n\
             sc description zapret \"Zapret DPI bypass software\"\r\n\
             echo Starting service...\r\n\
             sc start zapret\r\n\
             if %errorlevel% neq 0 (\r\n\
                 echo Failed to start service. Checking error...\r\n\
                 sc query zapret\r\n\
                 exit /b 1\r\n\
             )\r\n\
             echo Service started successfully\r\n\
             reg add \"HKLM\\System\\CurrentControlSet\\Services\\zapret\" /v zapret-discord-youtube /t REG_SZ /d \"{}\" /f\r\n",
             bin_dir_path.display(), bin_name, bin_str, args, strategy
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

            let output = cmd.output();
            match output {
                Ok(out) => {
                    if !out.status.success() {
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        return Err(format!("Ошибка запуска сервиса: {}", stderr));
                    }
                }
                Err(e) => {
                    return Err(format!("Не удалось запустить PowerShell: {}", e));
                }
            }
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

    *state.active_strategy.lock().unwrap() = Some(strategy.clone());
    *state.last_strategy.lock().unwrap() = Some(strategy);
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
        let _ = Command::new("powershell")
            .args([
                "-NoProfile",
                "-WindowStyle", "Hidden",
                "-Command",
                "Start-Process cmd.exe -ArgumentList '/c %TEMP%\\zapret_stop.bat' -Verb RunAs -Wait -WindowStyle Hidden",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }

    *state.active_strategy.lock().unwrap() = None;
}

// ─── User Lists Management ────────────────────────────────────────────────────

/// Reads lines from a file in the lists directory, filtering out comments and empty lines
#[tauri::command]
fn read_user_list(filename: String) -> Result<Vec<String>, String> {
    let dir = find_binaries_dir();
    let file_path = dir.join("lists").join(&filename);

    if !file_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read {}: {}", filename, e))?;

    let lines: Vec<String> = content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|s| s.to_string())
        .collect();

    Ok(lines)
}

/// Writes lines to a file in the lists directory
#[tauri::command]
fn write_user_list(filename: String, lines: Vec<String>) -> Result<(), String> {
    let dir = find_binaries_dir();
    let file_path = dir.join("lists").join(&filename);

    let content = lines.join("\r\n");
    std::fs::write(&file_path, content)
        .map_err(|e| format!("Failed to write {}: {}", filename, e))?;

    Ok(())
}

/// Adds a line to a user list file
#[tauri::command]
fn add_to_user_list(filename: String, entry: String) -> Result<(), String> {
    let dir = find_binaries_dir();
    let file_path = dir.join("lists").join(&filename);

    let mut lines = if file_path.exists() {
        let content = std::fs::read_to_string(&file_path)
            .map_err(|e| format!("Failed to read {}: {}", filename, e))?;
        content
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(|s| s.to_string())
            .collect::<Vec<String>>()
    } else {
        Vec::new()
    };

    // Check for duplicates
    let entry_trimmed = entry.trim();
    if !lines.iter().any(|l| l.trim() == entry_trimmed) {
        lines.push(entry_trimmed.to_string());
        let content = lines.join("\r\n");
        std::fs::write(&file_path, content)
            .map_err(|e| format!("Failed to write {}: {}", filename, e))?;
    }

    Ok(())
}

/// Removes a line from a user list file
#[tauri::command]
fn remove_from_user_list(filename: String, entry: String) -> Result<(), String> {
    let dir = find_binaries_dir();
    let file_path = dir.join("lists").join(&filename);

    if !file_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read {}: {}", filename, e))?;

    let entry_trimmed = entry.trim();
    let lines: Vec<String> = content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && line.trim() != entry_trimmed)
        .map(|s| s.to_string())
        .collect();

    let content = lines.join("\r\n");
    std::fs::write(&file_path, content)
        .map_err(|e| format!("Failed to write {}: {}", filename, e))?;

    Ok(())
}

/// Updates the IPSet list from remote source (same as service.bat)
#[tauri::command]
async fn update_ipset_list() -> Result<String, String> {
    let dir = find_binaries_dir();
    let list_file = dir.join("lists").join("ipset-all.txt");
    let url = "https://raw.githubusercontent.com/Flowseal/zapret-discord-youtube/refs/heads/main/.service/ipset-service.txt";

    // Check if curl exists in System32
    let curl_path = std::path::Path::new(r"C:\Windows\System32\curl.exe");
    let output = if curl_path.exists() {
        Command::new(curl_path)
            .args(["-L", "-o", list_file.to_str().unwrap_or(""), url])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    } else {
        // Fallback to PowerShell
        let ps_cmd = format!(
            "$url = '{}'; $out = '{}'; try {{ $res = Invoke-WebRequest -Uri $url -TimeoutSec 30 -UseBasicParsing; if ($res.StatusCode -eq 200) {{ $res.Content | Out-File -FilePath $out -Encoding UTF8 }} else {{ exit 1 }} }} catch {{ exit 1 }}",
            url,
            list_file.to_str().unwrap_or("")
        );
        Command::new("powershell")
            .args(["-NoProfile", "-Command", &ps_cmd])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    };

    match output {
        Ok(out) if out.status.success() => {
            // Count lines in the downloaded file
            let content = std::fs::read_to_string(&list_file)
                .map_err(|e| format!("Failed to read downloaded file: {}", e))?;
            let count = content.lines().filter(|l| !l.trim().is_empty()).count();
            Ok(format!("Updated successfully. {} IPs loaded.", count))
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            Err(format!("Failed to update IPSet list: {}", stderr))
        }
        Err(e) => Err(format!("Failed to execute update command: {}", e)),
    }
}



#[tauri::command]
async fn download_and_install_update(window: tauri::Window) -> Result<String, String> {
    let dir = find_binaries_dir();
    let temp_dir = std::env::temp_dir().join("zapret_update");

    // Create temp directory
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).map_err(|e| format!("Failed to create temp dir: {}", e))?;

    // Backup user files
    let lists_dir = dir.join("lists");
    let backup_dir = temp_dir.join("backup");
    std::fs::create_dir_all(&backup_dir).ok();

    if lists_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&lists_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.contains("user") {
                        let _ = std::fs::copy(entry.path(), backup_dir.join(name));
                    }
                }
            }
        }
    }

    window.emit("download-progress", 5).ok();

    // Fetch version
    let version_cmd = format!(
        "try {{ (Invoke-WebRequest -Uri '{}' -Headers @{{'Cache-Control'='no-cache'}} -UseBasicParsing -TimeoutSec 10).Content.Trim() }} catch {{ exit 1 }}",
        GITHUB_VERSION_URL
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-Command", &version_cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    let latest_version = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => return Err("Failed to fetch latest version tag".to_string()),
    };

    window.emit("download-progress", 10).ok();

    // Download — use the simple Invoke-WebRequest (proven reliable)
    let download_url = format!("https://github.com/Flowseal/zapret-discord-youtube/releases/download/{}/zapret-discord-youtube-{}.zip", latest_version, latest_version);
    let zip_path = temp_dir.join("update.zip");

    let ps_cmd = format!(
        "$ProgressPreference = 'SilentlyContinue'; \
         [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; \
         try {{ Invoke-WebRequest -Uri '{}' -OutFile '{}' -TimeoutSec 300 -UseBasicParsing; Write-Host 'DONE' }} catch {{ Write-Host ('ERR:' + $_.Exception.Message); exit 1 }}",
        download_url,
        zip_path.to_str().unwrap_or("")
    );

    // Spawn a background thread that sends fake progress ticks every 2s
    // Progress goes 10 → 88, then we jump to 92 after download completes
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let done_flag = Arc::new(AtomicBool::new(false));
    let done_flag_thread = done_flag.clone();
    let window_clone = window.clone();
    std::thread::spawn(move || {
        let steps: &[u16] = &[15, 20, 28, 35, 42, 50, 58, 65, 72, 78, 83, 88];
        for pct in steps {
            if done_flag_thread.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(3));
            if done_flag_thread.load(Ordering::Relaxed) {
                break;
            }
            window_clone.emit("download-progress", *pct).ok();
        }
    });

    let out = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    done_flag.store(true, Ordering::Relaxed);

    match out {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let stdout = String::from_utf8_lossy(&o.stdout);
            return Err(format!(
                "Download failed: {} {}",
                stderr.trim(),
                stdout.trim()
            ));
        }
        Err(e) => return Err(format!("Failed to launch download: {}", e)),
    }

    if !zip_path.exists() {
        return Err("Download failed: output file not found".to_string());
    }

    // Extraction
    window.emit("download-progress", 92).ok();
    let extract_dir = temp_dir.join("extracted");
    let _ = std::fs::create_dir_all(&extract_dir);

    let extract_cmd = format!(
        "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
        zip_path.to_str().unwrap_or(""),
        extract_dir.to_str().unwrap_or("")
    );

    let ex_status = Command::new("powershell")
        .args(["-NoProfile", "-Command", &extract_cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .status();

    if ex_status.is_err() || !ex_status.unwrap().success() {
        return Err("Extraction failed".to_string());
    }

    window.emit("download-progress", 95).ok();

    let mut extracted_folder = extract_dir.clone();
    if let Ok(entries) = std::fs::read_dir(&extract_dir) {
        let items: Vec<_> = entries.flatten().collect();
        if items.len() == 1 && items[0].path().is_dir() {
            extracted_folder = items[0].path();
        }
    }

    copy_dir_contents(&extracted_folder, &dir)?;

    // Restore
    let new_lists_dir = dir.join("lists");
    let _ = std::fs::create_dir_all(&new_lists_dir);
    if let Ok(entries) = std::fs::read_dir(&backup_dir) {
        for entry in entries.flatten() {
            let _ = std::fs::copy(entry.path(), new_lists_dir.join(entry.file_name()));
        }
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
    window.emit("download-progress", 100).ok();

    Ok("Update successful".to_string())
}

/// Recursively copies directory contents
fn copy_dir_contents(src: &PathBuf, dst: &PathBuf) -> Result<(), String> {
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let file_name = entry.file_name();
        let dest_path = dst.join(&file_name);

        if path.is_dir() {
            let _ = std::fs::create_dir_all(&dest_path);
            let _ = copy_dir_contents(&path, &dest_path);
        } else {
            if std::fs::copy(&path, &dest_path).is_err() {
                // If it fails (likely due to lock), try to rename the locked destination file first
                let mut old_path = dest_path.clone();
                let new_name = format!("{}.old", file_name.to_str().unwrap_or("locked"));
                old_path.set_file_name(new_name);
                let _ = std::fs::rename(&dest_path, &old_path); // ignore rename errors

                // Attempt copy again
                let _ = std::fs::copy(&path, &dest_path);
            }
        }
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct DiagnosticCheck {
    name: String,
    status: String, // "passed", "warning", "error"
    message: String,
    link: Option<String>,
}

#[derive(serde::Serialize)]
struct DiagnosticsResult {
    checks: Vec<DiagnosticCheck>,
    vpn_services: Option<String>,
}

/// Runs all diagnostic checks
#[tauri::command]
async fn run_diagnostics() -> Result<DiagnosticsResult, String> {
    let mut checks = Vec::new();
    let mut vpn_services: Option<String> = None;

    // 1. Base Filtering Engine check
    let bfe_check = Command::new("sc")
        .args(["query", "BFE"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match bfe_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.to_lowercase().contains("running") {
                checks.push(DiagnosticCheck {
                    name: "Base Filtering Engine".to_string(),
                    status: "passed".to_string(),
                    message: "Service is running".to_string(),
                    link: None,
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "Base Filtering Engine".to_string(),
                    status: "error".to_string(),
                    message: "Service is not running. This service is required for zapret to work"
                        .to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "Base Filtering Engine".to_string(),
                status: "error".to_string(),
                message: "Failed to check service status".to_string(),
                link: None,
            });
        }
    }

    // 2. Proxy check
    let proxy_check = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "try { $val = Get-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings' -Name ProxyEnable -ErrorAction Stop; if ($val.ProxyEnable -eq 1) { $srv = Get-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings' -Name ProxyServer -ErrorAction SilentlyContinue; Write-Host \"PROXY_ENABLED:$($srv.ProxyServer)\" } else { Write-Host \"PROXY_DISABLED\" } } catch { Write-Host \"PROXY_DISABLED\" }"
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match proxy_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.contains("PROXY_ENABLED:") {
                let proxy = stdout.split(':').nth(1).unwrap_or("unknown").trim();
                checks.push(DiagnosticCheck {
                    name: "System Proxy".to_string(),
                    status: "warning".to_string(),
                    message: format!("System proxy is enabled: {}. Make sure it's valid or disable it if you don't use a proxy", proxy),
                    link: None,
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "System Proxy".to_string(),
                    status: "passed".to_string(),
                    message: "No system proxy detected".to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "System Proxy".to_string(),
                status: "passed".to_string(),
                message: "Proxy check passed".to_string(),
                link: None,
            });
        }
    }

    // 3. TCP timestamps check
    let tcp_check = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "$out = netsh interface tcp show global; if ($out -match 'RFC 1323.*enabled') { Write-Host 'TIMESTAMPS_ENABLED' } else { Write-Host 'TIMESTAMPS_DISABLED' }"
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match tcp_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.contains("TIMESTAMPS_ENABLED") {
                checks.push(DiagnosticCheck {
                    name: "TCP Timestamps".to_string(),
                    status: "passed".to_string(),
                    message: "TCP timestamps are enabled".to_string(),
                    link: None,
                });
            } else {
                // Try to enable
                let _ = Command::new("netsh")
                    .args(["interface", "tcp", "set", "global", "timestamps=enabled"])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output();
                checks.push(DiagnosticCheck {
                    name: "TCP Timestamps".to_string(),
                    status: "warning".to_string(),
                    message: "TCP timestamps were disabled. Attempted to enable them.".to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "TCP Timestamps".to_string(),
                status: "warning".to_string(),
                message: "Failed to check TCP timestamps".to_string(),
                link: None,
            });
        }
    }

    // 4. Adguard check
    let adguard_check = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq AdguardSvc.exe", "/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match adguard_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.to_lowercase().contains("adguardsvc") {
                checks.push(DiagnosticCheck {
                    name: "Adguard".to_string(),
                    status: "error".to_string(),
                    message: "Adguard process found. Adguard may cause problems with Discord"
                        .to_string(),
                    link: Some(
                        "https://github.com/Flowseal/zapret-discord-youtube/issues/417".to_string(),
                    ),
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "Adguard".to_string(),
                    status: "passed".to_string(),
                    message: "Adguard not detected".to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "Adguard".to_string(),
                status: "passed".to_string(),
                message: "Adguard check passed".to_string(),
                link: None,
            });
        }
    }

    // 5. Killer services check
    let killer_check = Command::new("sc")
        .args(["query"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match killer_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.to_lowercase().contains("killer") {
                checks.push(DiagnosticCheck {
                    name: "Killer Network Service".to_string(),
                    status: "error".to_string(),
                    message: "Killer services found. Killer conflicts with zapret".to_string(),
                    link: Some("https://github.com/Flowseal/zapret-discord-youtube/issues/2512#issuecomment-2821119513".to_string()),
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "Killer Network Service".to_string(),
                    status: "passed".to_string(),
                    message: "Killer services not detected".to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "Killer Network Service".to_string(),
                status: "passed".to_string(),
                message: "Killer check passed".to_string(),
                link: None,
            });
        }
    }

    // 6. Intel Connectivity check
    let intel_check = Command::new("sc")
        .args(["query"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match intel_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_lowercase();
            if stdout.contains("intel") && stdout.contains("connectivity") {
                checks.push(DiagnosticCheck {
                    name: "Intel Connectivity Network Service".to_string(),
                    status: "error".to_string(),
                    message: "Intel Connectivity Network Service found. It conflicts with zapret"
                        .to_string(),
                    link: Some(
                        "https://github.com/ValdikSS/GoodbyeDPI/issues/541#issuecomment-2661670982"
                            .to_string(),
                    ),
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "Intel Connectivity Network Service".to_string(),
                    status: "passed".to_string(),
                    message: "Intel Connectivity service not detected".to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "Intel Connectivity Network Service".to_string(),
                status: "passed".to_string(),
                message: "Intel Connectivity check passed".to_string(),
                link: None,
            });
        }
    }

    // 7. Check Point check
    let checkpoint_check = Command::new("sc")
        .args(["query"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match checkpoint_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_lowercase();
            if stdout.contains("tracsrvwrapper") || stdout.contains("epwd") {
                checks.push(DiagnosticCheck {
                    name: "Check Point".to_string(),
                    status: "error".to_string(),
                    message: "Check Point services found. Check Point conflicts with zapret"
                        .to_string(),
                    link: None,
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "Check Point".to_string(),
                    status: "passed".to_string(),
                    message: "Check Point services not detected".to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "Check Point".to_string(),
                status: "passed".to_string(),
                message: "Check Point check passed".to_string(),
                link: None,
            });
        }
    }

    // 8. SmartByte check
    let smartbyte_check = Command::new("sc")
        .args(["query"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match smartbyte_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_lowercase();
            if stdout.contains("smartbyte") {
                checks.push(DiagnosticCheck {
                    name: "SmartByte".to_string(),
                    status: "error".to_string(),
                    message: "SmartByte services found. SmartByte conflicts with zapret"
                        .to_string(),
                    link: None,
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "SmartByte".to_string(),
                    status: "passed".to_string(),
                    message: "SmartByte services not detected".to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "SmartByte".to_string(),
                status: "passed".to_string(),
                message: "SmartByte check passed".to_string(),
                link: None,
            });
        }
    }

    // 9. VPN services check
    let vpn_check = Command::new("sc")
        .args(["query"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match vpn_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let vpn_lines: Vec<&str> = stdout
                .lines()
                .filter(|l| l.to_lowercase().contains("vpn"))
                .collect();
            if !vpn_lines.is_empty() {
                let services: Vec<String> = vpn_lines
                    .iter()
                    .filter_map(|l| l.split(':').nth(1))
                    .map(|s| s.trim().to_string())
                    .collect();
                vpn_services = Some(services.join(", "));
                checks.push(DiagnosticCheck {
                    name: "VPN Services".to_string(),
                    status: "warning".to_string(),
                    message: "VPN services found. Some VPNs can conflict with zapret".to_string(),
                    link: None,
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "VPN Services".to_string(),
                    status: "passed".to_string(),
                    message: "No VPN services detected".to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "VPN Services".to_string(),
                status: "passed".to_string(),
                message: "VPN check passed".to_string(),
                link: None,
            });
        }
    }

    // 10. DNS over HTTPS check
    let doh_check = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "try { $count = Get-ChildItem -Recurse -Path 'HKLM:System\\CurrentControlSet\\Services\\Dnscache\\InterfaceSpecificParameters\\' | Get-ItemProperty | Where-Object { $_.DohFlags -gt 0 } | Measure-Object | Select-Object -ExpandProperty Count; Write-Host \"DOH:$count\" } catch { Write-Host \"DOH:0\" }"
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match doh_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.contains("DOH:0") {
                checks.push(DiagnosticCheck {
                    name: "Secure DNS".to_string(),
                    status: "warning".to_string(),
                    message: "Make sure you have configured secure DNS in a browser with some non-default DNS service provider. If you use Windows 11 you can configure encrypted DNS in the Settings to hide this warning".to_string(),
                    link: None,
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "Secure DNS".to_string(),
                    status: "passed".to_string(),
                    message: "Secure DNS is configured".to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "Secure DNS".to_string(),
                status: "warning".to_string(),
                message: "Failed to check DNS configuration".to_string(),
                link: None,
            });
        }
    }

    // 11. Hosts file check
    let hosts_path = std::path::Path::new(r"C:\Windows\System32\drivers\etc\hosts");
    if hosts_path.exists() {
        if let Ok(content) = std::fs::read_to_string(hosts_path) {
            let content_lower = content.to_lowercase();
            if content_lower.contains("youtube.com") || content_lower.contains("youtu.be") {
                checks.push(DiagnosticCheck {
                    name: "Hosts File".to_string(),
                    status: "warning".to_string(),
                    message: "Your hosts file contains entries for youtube.com or youtu.be. This may cause problems with YouTube access".to_string(),
                    link: None,
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "Hosts File".to_string(),
                    status: "passed".to_string(),
                    message: "No YouTube entries in hosts file".to_string(),
                    link: None,
                });
            }
        }
    }

    // 12. WinDivert check
    let windivert_check = Command::new("sc")
        .args(["query", "WinDivert"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match windivert_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.to_lowercase().contains("running") {
                checks.push(DiagnosticCheck {
                    name: "WinDivert".to_string(),
                    status: "passed".to_string(),
                    message: "WinDivert driver is running".to_string(),
                    link: None,
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "WinDivert".to_string(),
                    status: "passed".to_string(),
                    message: "WinDivert driver not active (will be started when needed)"
                        .to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "WinDivert".to_string(),
                status: "passed".to_string(),
                message: "WinDivert check passed".to_string(),
                link: None,
            });
        }
    }

    Ok(DiagnosticsResult {
        checks,
        vpn_services,
    })
}

/// Clears Discord cache
#[tauri::command]
fn clear_discord_cache() -> Result<String, String> {
    let mut messages = Vec::new();

    // Check if Discord is running and close it
    let discord_processes = ["Discord.exe", "DiscordPTB.exe", "DiscordCanary.exe"];
    let mut discord_was_running = false;

    for process in &discord_processes {
        let check_output = Command::new("tasklist")
            .args([
                "/FI",
                &format!("IMAGENAME eq {}", process),
                "/FO",
                "CSV",
                "/NH",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        if let Ok(out) = check_output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.to_lowercase().contains(&process.to_lowercase()) {
                discord_was_running = true;
                messages.push(format!("Discord is running, closing {}...", process));

                // Kill the process
                let _ = Command::new("taskkill")
                    .args(["/F", "/IM", process])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output();
            }
        }
    }

    if discord_was_running {
        // Wait a bit for Discord to close
        std::thread::sleep(std::time::Duration::from_millis(1000));
        messages.push("Discord was successfully closed".to_string());
    }

    // Discord cache is in APPDATA (Roaming), not LOCALAPPDATA
    let appdata = std::env::var("APPDATA").map_err(|_| "Could not find APPDATA".to_string())?;

    let discord_paths = [
        format!("{}\\discord\\Cache", appdata),
        format!("{}\\discord\\Code Cache", appdata),
        format!("{}\\discord\\GPUCache", appdata),
        format!("{}\\DiscordPTB\\Cache", appdata),
        format!("{}\\DiscordPTB\\Code Cache", appdata),
        format!("{}\\DiscordPTB\\GPUCache", appdata),
        format!("{}\\DiscordCanary\\Cache", appdata),
        format!("{}\\DiscordCanary\\Code Cache", appdata),
        format!("{}\\DiscordCanary\\GPUCache", appdata),
    ];

    let mut cleared = 0;

    for path_str in &discord_paths {
        let path = std::path::Path::new(path_str);
        if path.exists() {
            // Count items before deletion for the message
            let mut items_deleted = 0;
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    let _ = std::fs::remove_dir_all(entry.path());
                    items_deleted += 1;
                }
            }

            if items_deleted > 0 {
                cleared += 1;
                messages.push(format!("Successfully deleted {}", path_str));
            }
        }
    }

    if cleared > 0 {
        Ok(messages.join("\n"))
    } else if discord_was_running {
        Ok(messages.join("\n"))
    } else {
        Ok("No Discord cache found to clear".to_string())
    }
}

/// Checks if running with administrator privileges
#[tauri::command]
fn check_admin_privileges() -> Result<bool, String> {
    Ok(is_admin())
}

#[derive(serde::Serialize)]
struct TestResult {
    config: String,
    status: String, // "success", "partial", "failed"
    http_ok: i32,
    http_error: i32,
    ping_ok: i32,
    ping_fail: i32,
}

#[derive(serde::Serialize)]
#[allow(dead_code)]
struct TestProgress {
    current: usize,
    total: usize,
    config_name: String,
}

/// Cancels a running test process
#[tauri::command]
fn cancel_tests(state: State<'_, AppState>) {
    let mut pid_lock = state.test_process_pid.lock().unwrap();
    if let Some(pid) = pid_lock.take() {
        // Kill process tree (/T = tree, /F = force)
        let _ = Command::new("taskkill")
            .arg("/F")
            .arg("/T")
            .arg("/PID")
            .arg(pid.to_string())
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    // Remove temp script if it still exists
    let temp_script = find_binaries_dir().join("utils").join("test_zapret_ui.ps1");
    let _ = std::fs::remove_file(&temp_script);
}

/// Runs configuration tests with real-time streaming output via Tauri events
#[tauri::command]
async fn run_tests(
    app: tauri::AppHandle,
    test_type: String,
    test_mode: String,
) -> Result<Vec<TestResult>, String> {
    let dir = find_binaries_dir();
    let utils_dir = dir.join("utils");
    let ps_script = utils_dir.join("test zapret.ps1");

    if !ps_script.exists() {
        return Err(
            "Test script not found. Please ensure zapret is properly installed.".to_string(),
        );
    }

    let original_content = std::fs::read_to_string(&ps_script)
        .map_err(|e| format!("Failed to read test script: {}", e))?;

    // Replace interactive function CALLS only (not definitions)
    let type_val = if test_type == "dpi" {
        "dpi"
    } else {
        "standard"
    };
    let modified_content = original_content
        .replace(
            "[void][System.Console]::ReadKey($true)",
            "# UI Mode - skipping ReadKey",
        )
        .replace(
            "$testType = Read-TestType",
            &format!("$testType = '{}'", type_val),
        )
        .replace("$mode = Read-ModeSelection", "$mode = 'all'")
        .replace(
            "    $selected = Read-ConfigSelection -allFiles $batFiles",
            "    $selected = $batFiles",
        )
        .replace(
            "    $batFiles = @($selected)",
            "    # UI Mode - using all configs",
        );

    let temp_script = utils_dir.join("test_zapret_ui.ps1");
    std::fs::write(&temp_script, modified_content)
        .map_err(|e| format!("Failed to write temp script: {}", e))?;

    let _ = app.emit(
        "test-progress",
        serde_json::json!({
            "line": format!("Starting {} tests ({} configs)...", type_val, test_mode),
            "kind": "info"
        }),
    );

    // Spawn the process and stream output line by line
    let mut child = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            temp_script.to_str().unwrap_or(""),
        ])
        .current_dir(&dir)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn test process: {}", e))?;

    // Store PID so cancel_tests / window-close can kill the process
    {
        let state = app.state::<AppState>();
        let mut pid_lock = state.test_process_pid.lock().unwrap();
        *pid_lock = Some(child.id());
    }

    let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
    let reader = BufReader::new(stdout);

    let mut all_lines: Vec<String> = Vec::new();

    for line_result in reader.lines() {
        if let Ok(raw) = line_result {
            // Strip ANSI color codes and trim
            let clean: String = raw.chars().filter(|c| c.is_ascii() || *c == '\n').collect();
            let line = clean.trim().to_string();
            if line.is_empty() {
                continue;
            }

            all_lines.push(line.clone());

            // Classify the line for coloring in the UI
            let kind = if line.contains("[ERROR]") || line.contains("[X]") {
                "error"
            } else if line.contains("[WARNING]") || line.contains("[WARN]") || line.contains("[?]")
            {
                "warning"
            } else if line.contains("[OK]")
                || line.contains("Best config:")
                || line.contains("Best strategy:")
            {
                "success"
            } else if line.contains("---") || line.contains("===") {
                "separator"
            } else if line.starts_with("  [") {
                "config"
            } else {
                "info"
            };

            let _ = app.emit(
                "test-progress",
                serde_json::json!({
                    "line": line,
                    "kind": kind
                }),
            );
        }
    }

    let _ = child.wait();

    // Clear PID — process finished (or was killed)
    {
        let state = app.state::<AppState>();
        let mut pid_lock = state.test_process_pid.lock().unwrap();
        *pid_lock = None;
    }

    // Clean up temp script
    let _ = std::fs::remove_file(&temp_script);

    // Parse analytics from accumulated lines
    let mut results = Vec::new();
    let mut in_analytics = false;

    for line in &all_lines {
        if line.contains("=== ANALYTICS ===") {
            in_analytics = true;
            continue;
        }
        if in_analytics && line.contains(".bat") {
            if let Some(config_name) = line.split(':').next() {
                let config = config_name.trim().to_string();
                let http_ok = extract_number(line, "HTTP OK:");
                let http_error = extract_number(line, "ERR:");
                let ping_ok = extract_number(line, "Ping OK:");
                let ping_fail = extract_number(line, "Fail:");

                let status = if http_error == 0 && ping_fail == 0 {
                    "success"
                } else if http_ok > http_error {
                    "partial"
                } else {
                    "failed"
                };

                results.push(TestResult {
                    config,
                    status: status.to_string(),
                    http_ok,
                    http_error,
                    ping_ok,
                    ping_fail,
                });
            }
        }
    }

    let _ = app.emit("test-done", serde_json::json!({ "count": results.len() }));

    Ok(results)
}

fn extract_number(text: &str, prefix: &str) -> i32 {
    if let Some(pos) = text.find(prefix) {
        let after = &text[pos + prefix.len()..];
        if let Some(end) = after.find(',') {
            after[..end].trim().parse().unwrap_or(0)
        } else {
            after.trim().parse().unwrap_or(0)
        }
    } else {
        0
    }
}

#[tauri::command]
fn update_tray_translations(
    translations: TrayTranslations,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) {
    {
        let mut lock = state.translations.lock().unwrap();
        *lock = Some(translations.clone());
    }

    // Update labels that don't depend on status
    if let Some(mi) = state.quit_item.lock().unwrap().as_ref() {
        let _ = mi.set_text(&translations.exit);
    }
    if let Some(mi) = state.show_item.lock().unwrap().as_ref() {
        let _ = mi.set_text(&translations.show);
    }
    if let Some(mi) = state.strategies_submenu.lock().unwrap().as_ref() {
        let _ = mi.set_text(&translations.change_strategy);
    }

    refresh_tray_menu(&app);
}

fn refresh_tray_menu(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let status = get_zapret_status(state.clone());
    let trans_lock = state.translations.lock().unwrap();
    let trans = match trans_lock.as_ref() {
        Some(t) => t,
        None => return, // Wait until translations are loaded
    };

    let status_mi = state.status_item.lock().unwrap().clone();
    if let Some(mi) = status_mi {
        let status_text = if status.running {
            &trans.status_on
        } else {
            &trans.status_off
        };
        let text = format!("{}{}", trans.status_prefix, status_text);
        let _ = mi.set_text(text);
    }

    let strategy_mi = state.strategy_item.lock().unwrap().clone();
    if let Some(mi) = strategy_mi {
        let text = format!(
            "{}{}",
            trans.strategy_prefix,
            status.strategy.as_deref().unwrap_or("---")
        );
        let _ = mi.set_text(text);
    }

    let toggle_mi = state.toggle_item.lock().unwrap().clone();
    if let Some(mi) = toggle_mi {
        let text = if status.running {
            &trans.toggle_off
        } else {
            &trans.toggle_on
        };
        let _ = mi.set_text(text);
    }
}

// ─── Entry point ──────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(windows)]
    elevate_if_needed();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            active_strategy: Mutex::new(None),
            test_process_pid: Mutex::new(None),
            status_item: Mutex::new(None),
            strategy_item: Mutex::new(None),
            toggle_item: Mutex::new(None),
            quit_item: Mutex::new(None),
            show_item: Mutex::new(None),
            strategies_submenu: Mutex::new(None),
            tray_handle: Mutex::new(None),
            notification_shown: AtomicBool::new(false),
            last_strategy: Mutex::new(None),
            translations: Mutex::new(None),
        })
        .setup(|app| {
            let quit_i = MenuItemBuilder::with_id("quit", "Exit").build(app)?;
            let show_i = MenuItemBuilder::with_id("show", "Restore window").build(app)?;

            let status_info = MenuItemBuilder::with_id("status_info", "Status: ---")
                .enabled(false)
                .build(app)?;
            let strategy_info = MenuItemBuilder::with_id("strategy_info", "Strategy: ---")
                .enabled(false)
                .build(app)?;
            let toggle_i = MenuItemBuilder::with_id("toggle", "Turn On Zapret").build(app)?;

            // Сохраняем ссылки для динамического обновления
            {
                let state = app.state::<AppState>();
                *state.status_item.lock().unwrap() = Some(status_info.clone());
                *state.strategy_item.lock().unwrap() = Some(strategy_info.clone());
                *state.toggle_item.lock().unwrap() = Some(toggle_i.clone());
                *state.quit_item.lock().unwrap() = Some(quit_i.clone());
                *state.show_item.lock().unwrap() = Some(show_i.clone());
            }

            // Загружаем стратегии
            let strategies = get_strategies().unwrap_or_default();
            let mut strategies_menu_builder = SubmenuBuilder::new(app, "Change strategy");
            for s in strategies {
                strategies_menu_builder = strategies_menu_builder
                    .item(&MenuItemBuilder::with_id(format!("strat_{}", s), s).build(app)?);
            }
            let strategies_submenu = strategies_menu_builder.build()?;
            {
                let state = app.state::<AppState>();
                *state.strategies_submenu.lock().unwrap() = Some(strategies_submenu.clone());
            }

            let menu = MenuBuilder::new(app)
                .item(&status_info)
                .item(&strategy_info)
                .separator()
                .item(&show_i)
                .item(&toggle_i)
                .item(&strategies_submenu)
                .separator()
                .item(&quit_i)
                .build()?;

            let tray = TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .on_menu_event(move |app, event| {
                    match event.id.as_ref() {
                        "quit" => {
                            app.exit(0);
                        }
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                                // Скрываем иконку при разворачивании
                                let state = app.state::<AppState>();
                                let tray_opt = state.tray_handle.lock().unwrap().clone();
                                if let Some(tray) = tray_opt {
                                    let _ = tray.set_visible(false);
                                }
                            }
                        }
                        "toggle" => {
                            let state = app.state::<AppState>();
                            let status = get_zapret_status(state.clone());
                            if status.running {
                                stop_zapret(state);
                            } else {
                                let last = state.last_strategy.lock().unwrap().clone();
                                let available = get_strategies().unwrap_or_default();
                                let strategy = last
                                    .or(status.strategy)
                                    .or_else(|| available.get(0).cloned());
                                if let Some(s) = strategy {
                                    let _ = start_zapret(s, "service".to_string(), state);
                                }
                            }
                            refresh_tray_menu(app);
                        }
                        id if id.starts_with("strat_") => {
                            let strategy = &id[6..];
                            let state = app.state::<AppState>();
                            let _ =
                                start_zapret(strategy.to_string(), "service".to_string(), state);
                            refresh_tray_menu(app);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    match event {
                        TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Left,
                            ..
                        } => {
                            let app = tray.app_handle();
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                                // Скрываем иконку при разворачивании
                                let _ = tray.set_visible(false);
                            }
                        }
                        TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Right,
                            ..
                        } => {
                            refresh_tray_menu(tray.app_handle());
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            // Сохраняем обработчик трея и скрываем его изначально
            {
                let state = app.state::<AppState>();
                let _ = tray.set_visible(false);
                *state.tray_handle.lock().unwrap() = Some(tray);
            }

            // Первоначальное обновление меню и детекция запущенной стратегии
            {
                let state = app.state::<AppState>();
                let status = get_zapret_status(state.clone());
                if status.running {
                    if let Some(s) = status.strategy {
                        *state.last_strategy.lock().unwrap() = Some(s);
                    }
                }
                refresh_tray_menu(app.handle());
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                window.hide().unwrap();
                api.prevent_close();

                // Показываем иконку при сворачивании в трей
                let state = window.app_handle().state::<AppState>();
                let tray_opt = state.tray_handle.lock().unwrap().clone();
                if let Some(tray) = tray_opt {
                    let _ = tray.set_visible(true);
                }

                // Показываем уведомление (один раз за сессию)
                if !state.notification_shown.swap(true, Ordering::SeqCst) {
                    let trans_lock = state.translations.lock().unwrap();
                    let (title, body) = match trans_lock.as_ref() {
                        Some(t) => (&t.minimized_title, &t.minimized_body),
                        None => (
                            &"Zapret minimized".to_string(),
                            &"The app is still running in the system tray.".to_string(),
                        ),
                    };

                    let _ = window
                        .app_handle()
                        .notification()
                        .builder()
                        .title(title)
                        .body(body)
                        .show();
                }

                // Kill any running test process when the window is closed
                let state = window.app_handle().state::<AppState>();
                let mut pid_lock = state.test_process_pid.lock().unwrap();
                if let Some(pid) = pid_lock.take() {
                    let _ = Command::new("taskkill")
                        .arg("/F")
                        .arg("/T")
                        .arg("/PID")
                        .arg(pid.to_string())
                        .creation_flags(CREATE_NO_WINDOW)
                        .output();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_strategies,
            get_local_version_cmd,
            get_ui_version_cmd,
            get_zapret_status,
            get_filters_status,
            set_game_filter,
            set_ipset_filter,
            start_zapret,
            stop_zapret,
            read_user_list,
            write_user_list,
            add_to_user_list,
            remove_from_user_list,
            update_ipset_list,
            get_remote_core_version,
            download_and_install_update,
            run_diagnostics,
            clear_discord_cache,
            check_admin_privileges,
            run_tests,
            check_status_full,
            ensure_binaries_present,
            cancel_tests,
            update_tray_translations,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
