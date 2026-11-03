use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::Manager;

/// 当前配置结构版本。旧版本壳写入的 config.json 没有此字段(读到 0),
/// 载入时做一次性迁移(见 load)。此后用户在设置里的修改按当前版本
/// 持久化,不再被纠正。
const CONFIG_REV: u32 = 2;

/// 应用配置:持久化到 app_config_dir/config.json。
/// 秘钥字段在首次启动服务时自动生成并回写。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// WeKnora API 监听地址
    pub host: String,
    /// WeKnora API 监听端口
    pub port: u16,
    /// 应用启动时自动拉起 Lite 服务
    pub auto_start: bool,
    /// 桌面应用退出后是否保留 Lite 服务进程
    pub keep_after_exit: bool,
    /// 后端程序绝对路径(linkmind-lite.exe);为空则按内置解析顺序查找
    pub lite_path: Option<String>,
    /// 服务工作目录(包含 web/ config/ migrations/ data/);为空则跟随 lite 可执行文件目录
    pub deploy_dir: Option<String>,
    /// 本地 Ollama 地址
    pub ollama_base_url: String,
    /// Lite 日志级别: debug / info / warn / error
    pub log_level: String,
    /// 额外注入到 Lite 进程的环境变量(覆盖同名的内置变量)
    pub extra_env: BTreeMap<String, String>,
    /// 随服务自动启停 docreader 文档解析服务(消除"未连接 gRPC"提示,启用 PDF 解析)。
    /// 默认开启:便携包自带 runtime/docreader(Python 运行时 + 依赖),检测不到
    /// 时启动流程仅记日志跳过,不影响 Lite 其余功能。
    pub docreader_enabled: bool,
    /// docreader 项目目录(含 main.py 与 .venv)
    pub docreader_dir: Option<String>,
    /// 凭据加密密钥(自动生成)
    pub system_aes_key: Option<String>,
    /// JWT 签名密钥(自动生成)
    pub jwt_secret: Option<String>,
    /// 自动更新源地址(latest.json 的 URL);为空使用内置默认(GitHub Releases)。
    /// 客户访问 GitHub 困难时可指向自有服务器/OSS 镜像,须 HTTPS。
    pub update_url: Option<String>,
    /// 配置结构版本;反序列化时缺失(旧版配置)则为 0
    #[serde(default)]
    pub config_rev: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 8080,
            auto_start: true,
            keep_after_exit: false,
            lite_path: None,
            deploy_dir: None,
            ollama_base_url: "http://127.0.0.1:11434".into(),
            log_level: "info".into(),
            extra_env: BTreeMap::new(),
            docreader_enabled: true,
            docreader_dir: None,
            system_aes_key: None,
            jwt_secret: None,
            update_url: None,
            config_rev: CONFIG_REV,
        }
    }
}

impl Config {
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    pub fn sanitize(&mut self) {
        if self.host.trim().is_empty() {
            self.host = "127.0.0.1".into();
        }
        if self.log_level.trim().is_empty() {
            self.log_level = "info".into();
        }
        if self.ollama_base_url.trim().is_empty() {
            self.ollama_base_url = "http://127.0.0.1:11434".into();
        }
        if self.port == 0 {
            self.port = 8080;
        }
        if let Some(u) = &self.update_url {
            if u.trim().is_empty() {
                self.update_url = None;
            } else {
                self.update_url = Some(u.trim().to_string());
            }
        }
    }
}

pub fn config_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("config.json"))
}

pub fn load(app: &tauri::AppHandle) -> Config {
    let Some(path) = config_path(app) else {
        return Config::default();
    };
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => return Config::default(),
    };
    let mut cfg: Config = serde_json::from_str(&text).unwrap_or_default();
    if cfg.config_rev < CONFIG_REV {
        // 旧版本壳的首次启动会把 docreader_enabled=false 固化进 config.json
        // (当时默认关闭),覆盖安装新版后解析服务因此不自启、引擎页显示
        // docreader 不可用。升级时一次性纠正为启用并补写版本号;之后用户
        // 的显式开关按当前版本持久化,不再被纠正。
        if !cfg.docreader_enabled {
            cfg.docreader_enabled = true;
        }
        cfg.config_rev = CONFIG_REV;
        let _ = save(app, &cfg);
    }
    cfg
}

pub fn save(app: &tauri::AppHandle, cfg: &Config) -> Result<(), String> {
    let Some(path) = config_path(app) else {
        return Err("无法定位配置目录".into());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    fs::write(&path, text).map_err(|e| e.to_string())
}

/// 生成 32 位十六进制随机串,用于 SYSTEM_AES_KEY / JWT_SECRET
pub fn random_hex() -> String {
    let mut buf = [0u8; 16];
    getrandom::fill(&mut buf).expect("system rng unavailable");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}
