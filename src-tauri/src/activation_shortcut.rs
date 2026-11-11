use serde::{Deserialize, Serialize};
use std::{fs, sync::Mutex};
use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    shortcut: String,
    active: bool,
    recording: bool,
    error: Option<String>,
    default_shortcut: String,
}
#[derive(Default)]
pub struct ActivationShortcut(Mutex<Status>);
#[derive(Serialize, Deserialize)]
struct Saved {
    shortcut: String,
}

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _, event| {
            if event.state != tauri_plugin_global_shortcut::ShortcutState::Pressed {
                return;
            }
            #[cfg(target_os = "macos")]
            let _ = app.show();
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        })
        .build()
}

fn default_shortcut() -> String {
    if cfg!(target_os = "macos") {
        "Alt+Super+KeyM"
    } else {
        "Control+Alt+KeyM"
    }
    .into()
}

fn parse(value: &str) -> Result<Option<Shortcut>, String> {
    if value.len() > 128 {
        return Err("快捷键组合无效".into());
    }
    if value.is_empty() {
        return Ok(None);
    }
    let parts: Vec<_> = value.split('+').collect();
    let mods = &parts[..parts.len().saturating_sub(1)];
    if parts.len() < 2
        || mods
            .iter()
            .any(|m| !["Control", "Alt", "Shift", "Super"].contains(m))
        || !mods.iter().any(|m| ["Control", "Alt", "Super"].contains(m))
        || mods.iter().enumerate().any(|(i, m)| mods[..i].contains(m))
    {
        return Err("请使用 Command、Ctrl 或 Alt 搭配其他按键".into());
    }
    value
        .parse()
        .map(Some)
        .map_err(|_| "不支持此快捷键组合".into())
}

fn write(app: &tauri::AppHandle, value: &str) -> Result<(), String> {
    use std::io::Write;
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|_| "无法找到设置目录")?;
    fs::create_dir_all(&dir).map_err(|_| "无法创建设置目录")?;
    let mut file = tempfile::NamedTempFile::new_in(&dir).map_err(|_| "无法保存唤起快捷键")?;
    let bytes = serde_json::to_vec(&Saved {
        shortcut: value.into(),
    })
    .map_err(|_| "快捷键设置无效")?;
    file.write_all(&bytes).map_err(|_| "无法保存唤起快捷键")?;
    file.as_file()
        .sync_all()
        .map_err(|_| "无法保存唤起快捷键")?;
    file.persist(dir.join("activation-shortcut.json"))
        .map_err(|_| "无法保存唤起快捷键")?;
    Ok(())
}

pub fn restore(app: &tauri::App) {
    let mut status = Status {
        default_shortcut: default_shortcut(),
        ..Status::default()
    };
    let saved = app
        .path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("activation-shortcut.json"));
    status.shortcut = match saved.filter(|path| path.exists()).map(fs::read) {
        None => status.default_shortcut.clone(),
        Some(Ok(bytes)) => match serde_json::from_slice::<Saved>(&bytes) {
            Ok(saved) => saved.shortcut,
            Err(_) => {
                status.error = Some("唤起快捷键设置损坏，请重新设置".into());
                String::new()
            }
        },
        Some(Err(_)) => {
            status.error = Some("无法读取唤起快捷键，请重新设置".into());
            String::new()
        }
    };
    if app.try_state::<tauri_plugin_global_shortcut::GlobalShortcut<tauri::Wry>>().is_none() {
        status.error = Some("当前系统无法使用全局快捷键".into());
    } else if !status.shortcut.is_empty() {
        match parse(&status.shortcut).and_then(|shortcut| {
            app.global_shortcut()
                .register(shortcut.unwrap())
                .map_err(|_| "唤起快捷键已被占用，请选择其他组合".into())
        }) {
            Ok(()) => status.active = true,
            Err(error) => status.error = Some(error),
        }
    }
    *app.state::<ActivationShortcut>().0.lock().unwrap() = status;
}

fn cancel(app: &tauri::AppHandle, status: &mut Status) {
    if !status.recording {
        return;
    }
    status.recording = false;
    if let Ok(Some(shortcut)) = parse(&status.shortcut) {
        match app.global_shortcut().register(shortcut) {
            Ok(()) => {
                status.active = true;
                status.error = None;
            }
            Err(_) => status.error = Some("唤起快捷键已被占用，请重新设置".into()),
        }
    }
}

pub fn cancel_recording(app: &tauri::AppHandle) {
    if let Ok(mut status) = app.state::<ActivationShortcut>().0.lock() {
        cancel(app, &mut status);
    }
}

// Register the replacement first so an occupied key cannot remove a working one.
fn replace(
    status: &mut Status,
    value: String,
    register: impl Fn(&str) -> Result<(), String>,
    unregister: impl Fn(&str) -> Result<(), String>,
    persist: impl Fn(&str) -> Result<(), String>,
) -> Result<(), String> {
    parse(&value)?;
    if status.shortcut == value && status.active {
        return Ok(());
    }
    if !value.is_empty() {
        register(&value)?;
    }
    if status.active && !status.shortcut.is_empty() {
        if let Err(error) = unregister(&status.shortcut) {
            if !value.is_empty() {
                let _ = unregister(&value);
            }
            return Err(error);
        }
    }
    if let Err(error) = persist(&value) {
        if !value.is_empty() {
            let _ = unregister(&value);
        }
        if status.active && !status.shortcut.is_empty() {
            status.active = register(&status.shortcut).is_ok();
        }
        return Err(error);
    }
    status.shortcut = value;
    status.active = !status.shortcut.is_empty();
    status.recording = false;
    status.error = None;
    Ok(())
}

#[tauri::command]
pub async fn activation_shortcut(
    app: tauri::AppHandle,
    action: String,
    shortcut: Option<String>,
) -> Result<Status, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<ActivationShortcut>();
        let mut status = state.0.lock().map_err(|_| "唤起快捷键暂时不可用")?;
        if action != "status" && app.try_state::<tauri_plugin_global_shortcut::GlobalShortcut<tauri::Wry>>().is_none() {
            return Err("当前系统无法使用全局快捷键".into());
        }
        match action.as_str() {
            "status" => {}
            "record" => {
                if !app.get_webview_window("settings").is_some_and(|window| {
                    window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false)
                }) {
                    return Err("请在设置窗口中录入快捷键".into());
                }
                if status.active {
                    app.global_shortcut()
                        .unregister(status.shortcut.as_str())
                        .map_err(|_| "无法开始录入快捷键")?;
                    status.active = false;
                }
                status.recording = true;
            }
            "cancel" => cancel(&app, &mut status),
            "set" => {
                let value = shortcut.ok_or("缺少快捷键组合")?;
                let result = replace(
                    &mut status,
                    value,
                    |value| {
                        app.global_shortcut()
                            .register(value)
                            .map_err(|_| "快捷键已被其他应用占用，请选择其他组合".into())
                    },
                    |value| {
                        app.global_shortcut()
                            .unregister(value)
                            .map_err(|_| "无法注销旧快捷键".into())
                    },
                    |value| write(&app, value),
                );
                if let Err(error) = result {
                    cancel(&app, &mut status);
                    return Err(error);
                }
            }
            _ => return Err("无效的快捷键操作".into()),
        }
        Ok(status.clone())
    })
    .await
    .map_err(|_| "唤起快捷键操作未完成".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[test]
    fn parses_default_clear_and_rejects_plain_typing() {
        for value in [
            "Alt+Super+KeyM",
            "Control+Alt+KeyM",
            "Control+Shift+Digit1",
            "",
        ] {
            assert!(parse(value).is_ok(), "{value}");
        }
        for value in ["KeyA", "Shift+KeyA", "Super+Super+KeyA", "Control+Unknown"] {
            assert!(parse(value).is_err(), "{value}");
        }
    }
    #[test]
    fn conflict_keeps_old_registration_and_failed_save_rolls_back() {
        let mut status = Status {
            shortcut: "Alt+Super+KeyM".into(),
            active: true,
            ..Status::default()
        };
        assert!(replace(
            &mut status,
            "Alt+Super+KeyN".into(),
            |_| Err("occupied".into()),
            |_| panic!(),
            |_| panic!()
        )
        .is_err());
        assert!(status.active);
        assert_eq!(status.shortcut, "Alt+Super+KeyM");
        let calls = RefCell::new(vec![]);
        assert!(replace(
            &mut status,
            "Alt+Super+KeyN".into(),
            |v| {
                calls.borrow_mut().push(format!("register:{v}"));
                Ok(())
            },
            |v| {
                calls.borrow_mut().push(format!("unregister:{v}"));
                Ok(())
            },
            |_| Err("full".into())
        )
        .is_err());
        assert!(status.active);
        assert_eq!(status.shortcut, "Alt+Super+KeyM");
        assert_eq!(calls.borrow().last().unwrap(), "register:Alt+Super+KeyM");
    }
    #[test]
    fn clearing_is_persisted_instead_of_restoring_the_default_on_restart() {
        let mut status = Status {
            shortcut: "Alt+Super+KeyM".into(),
            active: true,
            ..Status::default()
        };
        let saved = RefCell::new(String::new());
        replace(
            &mut status,
            "".into(),
            |_| panic!(),
            |_| Ok(()),
            |value| {
                *saved.borrow_mut() = serde_json::to_string(&Saved {
                    shortcut: value.into(),
                })
                .unwrap();
                Ok(())
            },
        )
        .unwrap();
        assert!(!status.active);
        assert!(!status.recording);
        assert_eq!(
            serde_json::from_str::<Saved>(&saved.borrow())
                .unwrap()
                .shortcut,
            ""
        );
    }
}
