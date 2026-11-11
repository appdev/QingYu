use serde::{Deserialize, Serialize};
#[cfg(any(target_os = "android", target_os = "ios"))]
use tauri::Manager;
use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CalendarEvent {
    pub title: String,
    pub notes: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub reminder_minutes: Option<u32>,
}

impl CalendarEvent {
    fn validate(&self) -> Result<(), String> {
        if self.title.trim().is_empty()
            || self.title.chars().count() > 200
            || self.title.chars().any(char::is_control)
        {
            return Err("请输入 1–200 字的日程标题".into());
        }
        if self.notes.chars().count() > 10_000 || self.notes.contains('\0') {
            return Err("备注不能超过 10000 字或包含空字符".into());
        }
        // Keep all native calendar backends within the same supported date range (1970–9999).
        if self.start_ms < 0 || self.end_ms <= self.start_ms || self.end_ms > 253_402_300_799_000 {
            return Err("请选择有效的起止时间，结束时间须晚于开始时间".into());
        }
        if self
            .reminder_minutes
            .is_some_and(|minutes| minutes > 40_320)
        {
            return Err("提醒时间须在日程开始前 0–40320 分钟内".into());
        }
        Ok(())
    }
}

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_calendar);

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("calendar")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            _app.manage(_api.register_android_plugin("app.miaoyan.calendar", "CalendarPlugin")?);
            #[cfg(target_os = "ios")]
            _app.manage(_api.register_ios_plugin(init_plugin_calendar)?);
            Ok(())
        })
        .build()
}

pub async fn create(app: tauri::AppHandle, event: CalendarEvent) -> Result<String, String> {
    event.validate()?;
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            let response: serde_json::Value = app
                .state::<tauri::plugin::PluginHandle<tauri::Wry>>()
                .run_mobile_plugin("createEvent", event)
                .map_err(|e| e.to_string())?;
            Ok(response["message"]
                .as_str()
                .unwrap_or("日程已写入系统日历")
                .to_string())
        }
        #[cfg(target_os = "macos")]
        {
            let _ = app;
            macos::create(event)
        }
        #[cfg(not(any(target_os = "macos", target_os = "android", target_os = "ios")))]
        {
            desktop::create(app, event)
        }
    })
    .await
    .map_err(|_| "创建日程未能完成".to_string())?
}

#[cfg(not(any(target_os = "macos", target_os = "android", target_os = "ios")))]
mod desktop;
#[cfg(any(
    test,
    not(any(target_os = "macos", target_os = "android", target_os = "ios"))
))]
mod ical;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(test)]
mod tests {
    use super::*;
    fn event() -> CalendarEvent {
        CalendarEvent {
            title: "会议".into(),
            notes: String::new(),
            start_ms: 1_800_000_000_000,
            end_ms: 1_800_003_600_000,
            reminder_minutes: Some(0),
        }
    }
    #[test]
    fn validates_native_boundary() {
        assert!(event().validate().is_ok());
        let mut e = event();
        e.title = " \n".into();
        assert!(e.validate().is_err());
        let mut e = event();
        e.end_ms = e.start_ms;
        assert!(e.validate().is_err());
        let mut e = event();
        e.reminder_minutes = Some(40_321);
        assert!(e.validate().is_err());
        let mut e = event();
        e.notes = "x\0".into();
        assert!(e.validate().is_err());
        let mut e = event();
        e.start_ms = -1;
        assert!(e.validate().is_err());
    }
}
