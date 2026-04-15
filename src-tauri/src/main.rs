use std::process::{Command, Child};
use std::sync::Mutex;
use tauri::State;

// Хранилище для запущенного процесса
struct AppState {
    child_process: Mutex<Option<Child>>,
}

#[tauri::command]
fn start_zapret(strategy: String, state: State<'_, AppState>) -> Result<String, String> {
    // 1. Убиваем старый процесс, если он есть
    let mut lock = state.child_process.lock().unwrap();
    if let Some(mut child) = lock.take() {
        let _ = child.kill();
    }

    // 2. Путь к winws.exe (нужно будет уточнить путь в продакшене)
    let child = Command::new("binaries/winws.exe")
        .args(strategy.split_whitespace()) // Разбиваем строку аргументов
        .spawn()
        .map_err(|e| e.to_string())?;

    *lock = Some(child);
    Ok("Connected".into())
}

#[tauri::command]
fn stop_zapret(state: State<'_, AppState>) {
    let mut lock = state.child_process.lock().unwrap();
    if let Some(mut child) = lock.take() {
        let _ = child.kill();
    }
}