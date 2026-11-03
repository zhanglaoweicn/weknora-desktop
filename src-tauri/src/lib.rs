mod config;
mod service;
mod update;

use serde::Serialize;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager, RunEvent,
};
use tauri_plugin_opener::OpenerExt;

use service::{ServiceState, StatusInfo};

#[derive(Debug, Clone, Serialize)]
pub struct HealthInfo {
    pub ok: bool,
    pub latency_ms: Option<u128>,
    pub error: Option<String>,
}

#[tauri::command]
fn get_config(state: tauri::State<'_, ServiceState>) -> config::Config {
    state.config()
}

#[tauri::command]
fn save_config(
    app: tauri::AppHandle,
    state: tauri::State<'_, ServiceState>,
    cfg: config::Config,
) -> Result<config::Config, String> {
    let mut cfg = cfg;
    cfg.sanitize();
    // 服务运行中不允许改端口/主机
    let status = state.current_status();
    let old = state.config();
    if (status == service::Status::Running || status == service::Status::Starting)
        && (old.port != cfg.port || old.host != cfg.host)
    {
        return Err("服务运行中无法修改监听地址,请先停止服务".into());
    }
    config::save(&app, &cfg)?;
    state.set_config(cfg.clone());
    Ok(cfg)
}

#[tauri::command]
fn get_status(state: tauri::State<'_, ServiceState>) -> StatusInfo {
    state.snapshot_pub()
}

#[tauri::command]
fn get_logs(state: tauri::State<'_, ServiceState>, limit: Option<usize>) -> Vec<service::LogLine> {
    let logs = state.logs_pub();
    let n = limit.unwrap_or(500).min(3000);
    logs.iter().rev().take(n).rev().cloned().collect()
}

#[tauri::command]
fn clear_logs(state: tauri::State<'_, ServiceState>) {
    state.clear_logs();
}

#[tauri::command]
fn start_service(app: tauri::AppHandle) -> Result<StatusInfo, String> {
    service::start(&app)
}

#[tauri::command]
fn stop_service(app: tauri::AppHandle) -> Result<StatusInfo, String> {
    service::stop(&app)
}

#[tauri::command]
fn restart_service(app: tauri::AppHandle) -> Result<StatusInfo, String> {
    service::restart(&app)
}

#[tauri::command]
fn health_check(state: tauri::State<'_, ServiceState>) -> HealthInfo {
    let cfg = state.config();
    match service::health_probe(&cfg.base_url()) {
        Ok(latency) => HealthInfo {
            ok: true,
            latency_ms: Some(latency),
            error: None,
        },
        Err(e) => HealthInfo {
            ok: false,
            latency_ms: None,
            error: Some(e),
        },
    }
}

#[tauri::command]
fn deploy_info(app: tauri::AppHandle) -> service::DeployInfo {
    service::deploy_info(&app)
}

#[tauri::command]
fn open_workbench(app: tauri::AppHandle) -> Result<String, String> {
    service::open_workbench(&app)
}

#[tauri::command]
fn open_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn open_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn reveal_data_dir(app: tauri::AppHandle) -> Result<(), String> {
    let info = service::deploy_info(&app);
    match info.data_dir {
        Some(dir) => app
            .opener()
            .open_path(dir, None::<&str>)
            .map_err(|e| e.to_string()),
        None => Err("尚未解析到部署目录".into()),
    }
}

fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let start = MenuItem::with_id(app, "start", "启动服务", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "停止服务", true, None::<&str>)?;
    let workbench = MenuItem::with_id(app, "workbench", "打开工作台", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &start, &stop, &workbench, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().expect("app icon").clone())
        .tooltip("LinkMind Desktop")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.unminimize();
                    let _ = win.set_focus();
                }
            }
            "start" => {
                let _ = service::start(app);
            }
            "stop" => {
                let _ = service::stop(app);
            }
            "workbench" => {
                let _ = service::open_workbench(app);
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.unminimize();
                    let _ = win.set_focus();
                }
            }
        })
        .build(app)?;
    Ok(())
}

pub fn run() {
    // WebView2: 显式豁免本机回环与 Tauri 应用源,系统代理(如 Clash)不得拦截。
    // 注意 <-loopback> 的语义是"取消回环豁免"(强制回环走代理),与意图相反;
    // 且 Tauri 在 Windows 上的应用源 http://tauri.localhost 不在隐式豁免内,
    // 被代理接管会表现为管控窗口"localhost 拒绝连接"。
    #[cfg(debug_assertions)]
    std::env::set_var(
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
        "--proxy-bypass-list=127.0.0.1;localhost;tauri.localhost;*.localhost --remote-debugging-port=9223",
    );
    #[cfg(not(debug_assertions))]
    std::env::set_var(
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
        "--proxy-bypass-list=127.0.0.1;localhost;tauri.localhost;*.localhost",
    );
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let cfg = config::load(app.handle());
            app.manage(ServiceState::new(cfg));
            app.manage(update::UpdateState {
                pending: parking_lot::Mutex::new(None),
            });
            build_tray(app.handle())?;

            // 预建隐藏的工作台窗口(规避 Windows 上运行时建窗的导航丢失/卡死缺陷)
            if let Err(e) = service::create_workbench_window(app.handle()) {
                eprintln!("workbench window prebuild failed: {e}");
            }

            // 首页事件管道: 日志行追加时通知前端
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                // 预热: 确保事件系统就绪(无实际作用,仅为结构清晰)
                let _ = handle.emit("service://ready", true);
            });

            // 配置了自动启动则拉起服务
            if app.state::<ServiceState>().config().auto_start {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(800));
                    if let Err(e) = service::start(&handle) {
                        let entry = handle
                            .state::<ServiceState>()
                            .push_log_sys(format!("自动启动失败: {e}"));
                        let _ = handle.emit(service::LOG_EVENT, &entry);
                        service::emit_status(&handle);
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            get_status,
            get_logs,
            clear_logs,
            start_service,
            stop_service,
            restart_service,
            health_check,
            deploy_info,
            open_workbench,
            open_url,
            open_path,
            reveal_data_dir,
            update::check_update,
            update::install_update
        ])
        .on_window_event(|window, event| {
            // 工作台窗口点关闭 = 隐藏,窗口常驻复用
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "workbench" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                service::shutdown_cleanup(app);
            }
        });
}
