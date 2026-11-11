use super::{ical, CalendarEvent};
use std::io::Write;
use tauri::Manager;

pub fn create(app: tauri::AppHandle, event: CalendarEvent) -> Result<String, String> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|_| "无法找到日历缓存目录")?
        .join("calendar");
    std::fs::create_dir_all(&dir).map_err(|_| "无法创建日历缓存目录")?;
    let mut file = tempfile::Builder::new()
        .prefix("event-")
        .suffix(".ics")
        .tempfile_in(dir)
        .map_err(|_| "无法创建日程文件")?;
    file.write_all(ical::encode(&event).as_bytes())
        .map_err(|_| "无法写入日程文件")?;
    let (_, path) = file.keep().map_err(|_| "无法保存日程文件")?;
    #[cfg(target_os = "windows")]
    let opened = {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                std::ptr::null(),
                path.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            ) as isize
                > 32
        }
    };
    #[cfg(not(target_os = "windows"))]
    let opened = std::process::Command::new("xdg-open")
        .arg(&path)
        .status()
        .is_ok_and(|status| status.success());
    if opened {
        Ok("已交给默认日历软件，请在其中确认导入，并检查提醒设置".into())
    } else {
        let _ = std::fs::remove_file(path);
        Err("无法打开日历软件，请先安装支持 .ics 日程的应用并设置为默认打开方式".into())
    }
}
