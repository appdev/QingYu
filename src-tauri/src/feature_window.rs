#[cfg(desktop)]
use tauri::{Emitter, Manager};

#[cfg(desktop)]
pub(crate) enum WindowChrome {
    Fixed,
    Resizable { min_width: f64, min_height: f64 },
}

#[cfg(desktop)]
pub(crate) struct FeatureWindow<'a> {
    pub label: &'a str,
    pub title: &'a str,
    pub route: String,
    pub size: (f64, f64),
    pub chrome: WindowChrome,
}

#[cfg(desktop)]
impl FeatureWindow<'_> {
    pub fn open(
        self,
        app: &tauri::AppHandle,
        prepare: impl FnOnce(&tauri::WebviewWindow, bool) -> Result<(), String>,
    ) -> Result<(tauri::WebviewWindow, bool), String> {
        if let Some(window) = app.get_webview_window(self.label) {
            prepare(&window, false)?;
            window.show().map_err(|e| e.to_string())?;
            window.set_focus().map_err(|e| e.to_string())?;
            return Ok((window, false));
        }
        let builder = tauri::WebviewWindowBuilder::new(
            app,
            self.label,
            tauri::WebviewUrl::App(self.route.into()),
        )
        .title(self.title)
        .inner_size(self.size.0, self.size.1)
        .center()
        .visible(false);
        let builder = match self.chrome {
            WindowChrome::Fixed => {
                let builder = builder
                    .resizable(false)
                    .maximizable(false)
                    .minimizable(false);
                #[cfg(target_os = "macos")]
                let builder = builder
                    .title_bar_style(tauri::TitleBarStyle::Overlay)
                    .hidden_title(true)
                    .traffic_light_position(tauri::LogicalPosition::new(18., 28.));
                builder
            }
            WindowChrome::Resizable {
                min_width,
                min_height,
            } => builder.min_inner_size(min_width, min_height),
        };
        let window = builder.build().map_err(|e| e.to_string())?;
        if let Err(error) = prepare(&window, true) {
            let _ = window.destroy();
            return Err(error);
        }
        crate::file_library::workspace::show_when_ready(app, self.label);
        Ok((window, true))
    }
}

pub(crate) fn is_picture_window(label: &str) -> bool {
    label
        .strip_prefix("feature-picture-")
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
}

#[tauri::command]
pub async fn open_feature_window(
    window: tauri::WebviewWindow,
    label: String,
) -> Result<(), String> {
    #[cfg(mobile)]
    {
        let _ = (window, label);
        Err("请使用应用内面板".into())
    }
    #[cfg(desktop)]
    {
        let app = window.app_handle().clone();
        let owner = window.label().to_owned();
        if !is_picture_window(&label)
            || (owner != "main" && !crate::file_library::workspace::is_document(&app, &owner))
        {
            return Err("无法从此窗口打开功能面板".into());
        }
        tauri::async_runtime::spawn_blocking(move || {
            let app_for_close = app.clone();
            let closed_label = label.clone();
            FeatureWindow {
                label: &label,
                title: "分享为图片",
                route: format!("feature.html?owner={owner}"),
                size: (560., 720.),
                chrome: WindowChrome::Resizable {
                    min_width: 340.,
                    min_height: 400.,
                },
            }
            .open(&app, |panel, created| {
                if created {
                    panel.on_window_event(move |event| {
                        if matches!(event, tauri::WindowEvent::Destroyed) {
                            let _ = app_for_close.emit_to(
                                owner.as_str(),
                                "feature-window-closed",
                                &closed_label,
                            );
                        }
                    });
                }
                Ok(())
            })?;
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?
    }
}
