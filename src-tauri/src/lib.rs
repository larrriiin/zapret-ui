use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use tauri::State;

struct AppState {
    active_strategy: Mutex<Option<String>>,
}

#[derive(serde::Serialize)]
struct ZapretStatus {
    running: bool,
    strategy: Option<String>,
}

/// Пытается найти папку binaries рядом с exe (продакшен)
/// или относительно рабочей директории (режим разработки).
fn find_binaries_dir() -> PathBuf {
    // Сначала проверяем рядом с исполняемым файлом (продакшен)
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..4 {
            if let Some(d) = dir {
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

    // Фолбэк: относительно рабочей директории (работает при `tauri dev`)
    PathBuf::from("binaries")
}

/// Проверяет, запущен ли winws.exe через tasklist
fn is_winws_running() -> bool {
    let output = Command::new("tasklist")
        .args(["/fi", "IMAGENAME eq winws.exe", "/fo", "csv", "/nh"])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.to_lowercase().contains("winws.exe")
        }
        Err(_) => false,
    }
}

/// Возвращает список стратегий — названия .bat файлов из папки binaries (без service.bat)
#[tauri::command]
fn get_strategies() -> Result<Vec<String>, String> {
    let binaries_dir = find_binaries_dir();

    let entries = std::fs::read_dir(&binaries_dir)
        .map_err(|e| format!("Не удалось прочитать папку binaries ({:?}): {}", binaries_dir, e))?;

    let mut strategies: Vec<String> = entries
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

    strategies.sort();
    Ok(strategies)
}

/// Возвращает текущее состояние zapret: запущен ли и какая стратегия активна
#[tauri::command]
fn get_zapret_status(state: State<'_, AppState>) -> ZapretStatus {
    let running = is_winws_running();
    let mut strategy_lock = state.active_strategy.lock().unwrap();

    if !running {
        // Если процесс завершился сам — сбрасываем запомненную стратегию
        *strategy_lock = None;
    }

    ZapretStatus {
        running,
        strategy: strategy_lock.clone(),
    }
}

/// Запускает указанную стратегию (по имени .bat файла без расширения)
#[tauri::command]
fn start_zapret(strategy: String, state: State<'_, AppState>) -> Result<String, String> {
    // Убиваем уже запущенный winws.exe, если есть
    let _ = Command::new("taskkill")
        .args(["/f", "/im", "winws.exe"])
        .output();

    let binaries_dir = find_binaries_dir();
    let bat_path = binaries_dir.join(format!("{}.bat", strategy));

    if !bat_path.exists() {
        return Err(format!("Файл стратегии не найден: {}.bat", strategy));
    }

    let bat_str = bat_path
        .to_str()
        .ok_or("Невалидный путь к bat-файлу")?
        .to_string();

    Command::new("cmd")
        .args(["/c", &bat_str])
        .spawn()
        .map_err(|e| format!("Не удалось запустить стратегию: {}", e))?;

    *state.active_strategy.lock().unwrap() = Some(strategy);
    Ok("Connected".into())
}

/// Останавливает zapret через taskkill
#[tauri::command]
fn stop_zapret(state: State<'_, AppState>) {
    let _ = Command::new("taskkill")
        .args(["/f", "/im", "winws.exe"])
        .output();

    *state.active_strategy.lock().unwrap() = None;
}

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
            start_zapret,
            stop_zapret
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
