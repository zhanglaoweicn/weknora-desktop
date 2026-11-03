//! 应用自动更新(安装版)。
//!
//! 基于 tauri-plugin-updater:更新清单(latest.json)与安装包随 GitHub
//! Releases 发布,客户端校验 minisign 签名后静默覆盖安装(安装/卸载钩子
//! 会先清场,见 installer-hooks.nsh)。
//!
//! 便携版(U 盘)没有安装器概念,更新包会装进 Program Files 而不是 U 盘,
//! 因此识别到便携布局时禁用更新,提示手动替换文件。

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

use crate::config::Config;

pub struct UpdateState {
    /// check_update 命中后暂存,install_update 再取出执行下载安装。
    /// Update 不可 Clone,也不能跨 await 序列化,只能驻留内存。
    pub pending: parking_lot::Mutex<Option<tauri_plugin_updater::Update>>,
}

/// exe 同目录下存在 runtime\linkmind-lite.exe 即视为便携布局
/// (安装版的资源在 <安装目录>\binaries\ 下,不会命中)。
fn is_portable_install() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Some(dir) = exe.parent() else {
        return false;
    };
    let runtime = dir.join("runtime");
    runtime.join("linkmind-lite.exe").is_file() || runtime.join("weknora-lite.exe").is_file()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMeta {
    pub available: bool,
    /// 便携布局:更新被禁用,前端据此展示"手动替换文件"提示
    pub portable: bool,
    pub current_version: String,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub date: Option<String>,
}

fn updater_builder(
    app: &AppHandle,
    cfg: &Config,
) -> Result<tauri_plugin_updater::UpdaterBuilder, String> {
    let builder = app.updater_builder();
    // 设置里配了自定义更新源(镜像/OSS)则以它为准,否则用 tauri.conf 内置端点
    if let Some(url) = cfg.update_url.as_deref() {
        let url = tauri::Url::parse(url).map_err(|e| format!("更新源地址无效: {e}"))?;
        return builder
            .endpoints(vec![url])
            .map_err(|e| format!("更新源地址无效: {e}"));
    }
    Ok(builder)
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<UpdateMeta, String> {
    let current = app.package_info().version.to_string();

    if is_portable_install() {
        return Ok(UpdateMeta {
            available: false,
            portable: true,
            current_version: current,
            version: None,
            notes: None,
            date: None,
        });
    }

    let cfg = app.state::<crate::service::ServiceState>().config();
    let update = updater_builder(&app, &cfg)?
        .build()
        .map_err(|e| format!("更新组件初始化失败: {e}"))?
        .check()
        .await
        .map_err(|e| format!("检查更新失败: {e}"))?;

    let Some(update) = update else {
        return Ok(UpdateMeta {
            available: false,
            portable: false,
            current_version: current,
            version: None,
            notes: None,
            date: None,
        });
    };
    let meta = UpdateMeta {
        available: true,
        portable: false,
        current_version: current,
        version: Some(update.version.clone()),
        notes: update.body.clone(),
        date: update.date.map(|d| d.to_string()),
    };
    let state = app.state::<UpdateState>();
    *state.pending.lock() = Some(update);
    Ok(meta)
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let update = app.state::<UpdateState>().pending.lock().take();
    let Some(update) = update else {
        return Err("请先检查更新".into());
    };

    #[derive(Clone, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Progress {
        stage: &'static str,
        downloaded: u64,
        total: Option<u64>,
    }

    let handle = app.clone();
    std::thread::spawn(move || {
        let app = handle;
        let result = tauri::async_runtime::block_on(async {
            let mut downloaded: u64 = 0;
            update
                .download_and_install(
                    |chunk, total| {
                        downloaded += chunk as u64;
                        let _ = app.emit(
                            "update://progress",
                            Progress {
                                stage: "downloading",
                                downloaded,
                                total,
                            },
                        );
                    },
                    || {
                        let _ = app.emit(
                            "update://progress",
                            Progress {
                                stage: "installing",
                                downloaded: 0,
                                total: None,
                            },
                        );
                    },
                )
                .await
        });
        match result {
            Ok(()) => {
                let _ = app.emit(
                    "update://progress",
                    Progress {
                        stage: "done",
                        downloaded: 0,
                        total: None,
                    },
                );
                // NSIS 安装器接管前先退出本应用;安装完成后用户重新打开即是新版本
                app.restart();
            }
            Err(e) => {
                let _ = app.emit(
                    "update://progress",
                    Progress {
                        stage: "failed",
                        downloaded: 0,
                        total: None,
                    },
                );
                let entry = app
                    .state::<crate::service::ServiceState>()
                    .push_log_sys(format!("自动更新失败: {e}"));
                let _ = app.emit(crate::service::LOG_EVENT, &entry);
            }
        }
    });
    Ok(())
}
