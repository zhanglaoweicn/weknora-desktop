use std::collections::{BTreeMap, VecDeque};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::config::{self, Config};

pub const LOG_EVENT: &str = "service://log";
pub const STATUS_EVENT: &str = "service://status";
const LOG_CAP: usize = 3000;
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);
const STARTUP_HEALTH_WINDOW: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Stopped => "已停止",
            Status::Starting => "启动中",
            Status::Running => "运行中",
            Status::Stopping => "停止中",
            Status::Failed => "启动失败",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub ts: u64,
    pub stream: String,
    pub line: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusInfo {
    pub status: Status,
    pub status_text: String,
    pub pid: Option<u32>,
    pub host: String,
    pub port: u16,
    pub base_url: String,
    pub started_at: Option<u64>,
    pub uptime_secs: Option<u64>,
    pub health_ok: Option<bool>,
    pub health_latency_ms: Option<u128>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeployInfo {
    pub lite_path: Option<String>,
    pub lite_found: bool,
    pub deploy_dir: Option<String>,
    pub web_ready: bool,
    pub config_ready: bool,
    pub migrations_ready: bool,
    pub data_dir: Option<String>,
    pub docreader_ready: bool,
}

pub struct ServiceState {
    docreader: Mutex<Option<Child>>,
    cfg: Mutex<Config>,
    child: Mutex<Option<Child>>,
    status: Mutex<Status>,
    logs: Mutex<VecDeque<LogLine>>,
    log_file: Mutex<Option<std::fs::File>>,
    started_at: Mutex<Option<SystemTime>>,
    last_error: Mutex<Option<String>>,
    health: Mutex<(Option<bool>, Option<u128>)>,
    generation: Mutex<u64>,
    /// 串行化 start():启动流程里有 taskkill/固定休眠等慢步骤,状态位
    /// 原本在拉起子进程之后才翻转为 Starting,并发触发(自动启动线程、
    /// 托盘菜单、控制台按钮)会双双穿过检查,拉出两个 Lite 实例——抢不到
    /// 端口的那个重试 30s 后以退出码 1 自杀,表现为"服务运行中意外终止"。
    start_lock: Mutex<()>,
}

impl ServiceState {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg: Mutex::new(cfg),
            docreader: Mutex::new(None),
            child: Mutex::new(None),
            status: Mutex::new(Status::Stopped),
            logs: Mutex::new(VecDeque::with_capacity(LOG_CAP)),
            log_file: Mutex::new(None),
            started_at: Mutex::new(None),
            last_error: Mutex::new(None),
            health: Mutex::new((None, None)),
            generation: Mutex::new(0),
            start_lock: Mutex::new(()),
        }
    }

    pub fn config(&self) -> Config {
        self.cfg.lock().clone()
    }

    pub fn set_config(&self, cfg: Config) {
        *self.cfg.lock() = cfg;
    }

    pub fn snapshot_pub(&self) -> StatusInfo {
        self.snapshot()
    }

    pub fn current_status(&self) -> Status {
        *self.status.lock()
    }

    pub fn logs_pub(&self) -> Vec<LogLine> {
        self.logs.lock().iter().cloned().collect()
    }

    pub fn clear_logs(&self) {
        self.logs.lock().clear();
    }

    /// 启动流程之外的系统级日志(如自动启动失败)
    pub fn push_log_sys(&self, line: String) -> LogLine {
        self.push_log("sys", line)
    }

    fn push_log(&self, stream: &str, line: String) -> LogLine {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or_default();
        let entry = LogLine {
            ts,
            stream: stream.into(),
            line: line.clone(),
        };
        let mut logs = self.logs.lock();
        if logs.len() >= LOG_CAP {
            logs.pop_front();
        }
        logs.push_back(entry.clone());
        drop(logs);
        if let Some(f) = &mut *self.log_file.lock() {
            let _ = writeln!(f, "{ts} [{stream}] {line}");
        }
        entry
    }

    fn snapshot(&self) -> StatusInfo {
        let cfg = self.cfg.lock().clone();
        let status = *self.status.lock();
        let pid = self.child.lock().as_ref().map(|c| c.id());
        let started_at = *self.started_at.lock();
        let uptime_secs = started_at.map(|t| {
            t.elapsed().map(|d| d.as_secs()).unwrap_or_default()
        });
        let (health_ok, health_latency_ms) = *self.health.lock();
        StatusInfo {
            status,
            status_text: status.as_str().into(),
            pid,
            host: cfg.host.clone(),
            port: cfg.port,
            base_url: cfg.base_url(),
            started_at: started_at.map(|t| {
                t.duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or_default()
            }),
            uptime_secs,
            health_ok,
            health_latency_ms,
            last_error: self.last_error.lock().clone(),
        }
    }
}

/// 读取子进程输出流并推送日志事件。
///
/// 读错误(如外部安全软件短暂干扰管道)绝不丢弃读取端：读取端一旦被关闭,
/// 子进程之后每次写 stdout/stderr 都得到 EPIPE——lite 的 logrus 会因此向
/// stderr 持续打印 "Failed to write to log, write /dev/stdout: The pipe is
/// being closed.",把服务日志面板刷满。这里改为提示一次后每秒重试,保持
/// 管道存活;子进程真正退出时读到 EOF 自然结束。
fn pump<R: std::io::Read + Send + 'static>(
    app: AppHandle,
    stream_name: &'static str,
    reader: R,
) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut warned = false;
        loop {
            let mut raw: Vec<u8> = Vec::new();
            match reader.read_until(b'\n', &mut raw) {
                Ok(0) => break, // EOF: 子进程关闭了它的写端(通常是退出)
                Ok(_) => {
                    warned = false;
                    let line = String::from_utf8_lossy(&raw);
                    let entry = app.state::<ServiceState>().push_log(
                        stream_name,
                        line.trim_end_matches(['\r', '\n']).to_string(),
                    );
                    let _ = app.emit(LOG_EVENT, &entry);
                }
                Err(e) => {
                    if !warned {
                        warned = true;
                        let entry = app.state::<ServiceState>().push_log(
                            "sys",
                            format!("{stream_name} 日志管道读取中断({e}),已保持管道并将自动重试"),
                        );
                        let _ = app.emit(LOG_EVENT, &entry);
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        }
    });
}

pub fn emit_status(app: &AppHandle) {
    let info = app.state::<ServiceState>().snapshot();
    let _ = app.emit(STATUS_EVENT, info);
}

/// 后端可执行文件:新包为 linkmind-lite.exe;旧包(weknora-lite.exe)兼容查找,
/// 保证覆盖升级时旧部署目录仍可启动。
fn lite_exe_path(dir: &PathBuf) -> Option<PathBuf> {
    let new = dir.join("linkmind-lite.exe");
    if new.is_file() {
        return Some(new);
    }
    let old = dir.join("weknora-lite.exe");
    if old.is_file() {
        return Some(old);
    }
    None
}

/// 定位 weknora-lite.exe 与其部署目录。
/// 解析顺序: 显式配置 > 打包资源 binaries/ > 开发目录 runtime/。
pub fn resolve_deploy(app: &AppHandle, cfg: &Config) -> Option<PathBuf> {
    if let Some(dir) = cfg.deploy_dir.as_deref() {
        let dir = PathBuf::from(dir);
        if lite_exe_path(&dir).is_some() {
            return Some(dir);
        }
    }
    if let Some(p) = cfg.lite_path.as_deref() {
        let p = PathBuf::from(p);
        if p.is_file() {
            return p.parent().map(|d| d.to_path_buf());
        }
    }
    // 开发环境: 从可执行文件目录向上最多 4 级查找 runtime/ 下的后端程序
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|d| d.to_path_buf());
        for _ in 0..4 {
            let Some(d) = dir else { break };
            let candidate = d.join("runtime");
            if lite_exe_path(&candidate).is_some() {
                return Some(candidate);
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }
    if let Ok(res) = app.path().resource_dir() {
        let dir = res.join("binaries");
        if lite_exe_path(&dir).is_some() {
            return Some(dir);
        }
    }
    None
}

pub fn deploy_info(app: &AppHandle) -> DeployInfo {
    let cfg = app.state::<ServiceState>().config();
    let dir = resolve_deploy(app, &cfg);
    match dir {
        Some(dir) => {
            let lite = lite_exe_path(&dir);
            DeployInfo {
                lite_found: lite.is_some(),
                lite_path: lite.map(|p| p.display().to_string()),
                deploy_dir: Some(dir.display().to_string()),
                web_ready: dir.join("web").join("index.html").is_file(),
                config_ready: dir.join("config").join("config.yaml").is_file()
                    || dir.join("config.yaml").is_file(),
                migrations_ready: dir.join("migrations").join("sqlite").is_dir(),
                data_dir: Some(dir.join("data").display().to_string()),
                docreader_ready: resolve_docreader_dir(&cfg, &dir).is_some(),
            }
        }
        None => DeployInfo {
            lite_path: None,
            lite_found: false,
            deploy_dir: None,
            web_ready: false,
            config_ready: false,
            migrations_ready: false,
            data_dir: None,
            docreader_ready: false,
        },
    }
}

/// 定位 docreader 项目目录。显式配置(docreader_dir)优先;否则使用部署目录
/// 自带的 docreader/(由 scripts/assemble-docreader.sh 随包分发)。
/// 只认服务入口确实存在的目录;两种布局都支持:
///   便携布局: dir/docreader/main.py(dir 是包的父目录)
///   开发布局: dir/main.py(dir 自身就是包目录,即 WeKnora 仓库的 docreader/)
fn resolve_docreader_dir(cfg: &Config, deploy_dir: &PathBuf) -> Option<PathBuf> {
    fn has_entry(dir: &PathBuf) -> bool {
        dir.join("main.py").is_file() || dir.join("docreader").join("main.py").is_file()
    }
    if let Some(d) = cfg.docreader_dir.as_deref().map(PathBuf::from) {
        if has_entry(&d) {
            return Some(d);
        }
    }
    let bundled = deploy_dir.join("docreader");
    if has_entry(&bundled) {
        return Some(bundled);
    }
    None
}

fn build_env(cfg: &Config, deploy_dir: &PathBuf) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert("GIN_MODE".into(), "release".into());
    env.insert("LOG_LEVEL".into(), cfg.log_level.clone());
    env.insert("SERVER_HOST".into(), cfg.host.clone());
    env.insert("SERVER_PORT".into(), cfg.port.to_string());
    env.insert("DB_DRIVER".into(), "sqlite".into());
    env.insert("DB_PATH".into(), "data/weknora.db".into());
    env.insert("RETRIEVE_DRIVER".into(), "sqlite".into());
    env.insert("STORAGE_TYPE".into(), "local".into());
    env.insert("LOCAL_STORAGE_BASE_DIR".into(), "data/files".into());
    env.insert("STREAM_MANAGER_TYPE".into(), "memory".into());
    env.insert("OLLAMA_BASE_URL".into(), cfg.ollama_base_url.clone());
    env.insert("NEO4J_ENABLE".into(), "false".into());
    env.insert("ENABLE_GRAPH_RAG".into(), "false".into());
    env.insert("WEKNORA_SANDBOX_MODE".into(), "disabled".into());
    env.insert("DISABLE_REGISTRATION".into(), "false".into());
    env.insert("AUTO_MIGRATE".into(), "true".into());
    env.insert("WEKNORA_WEB_DIR".into(), deploy_dir.join("web").display().to_string());
    // 结巴分词词典:gojieba 的默认词典路径是编译期写死的绝对路径(编译机
    // 模块缓存位置),换机必炸。部署目录自带 dict/ 时按相对工作目录注入
    // JIEBA_DICT_DIR(与 DB_PATH 同约定);extra_env 同名变量仍可覆盖。
    if !env.contains_key("JIEBA_DICT_DIR")
        && deploy_dir.join("dict").join("jieba.dict.utf8").is_file()
    {
        env.insert("JIEBA_DICT_DIR".into(), "dict".into());
    }
    // docreader 解析服务的 gRPC 地址(ensure_docreader 负责拉起,先于 Lite)。
    // 不注入时 Lite 永远不连解析服务,PDF 等复杂格式一律报"引擎不可用"。
    // extra_env 里的同名变量优先生效(用户可指向外部 docreader)。
    if cfg.docreader_enabled && !env.contains_key("DOCREADER_ADDR") {
        env.insert("DOCREADER_ADDR".into(), "127.0.0.1:50051".into());
    }
    if let Some(k) = &cfg.system_aes_key {
        env.insert("SYSTEM_AES_KEY".into(), k.clone());
    }
    if let Some(s) = &cfg.jwt_secret {
        env.insert("JWT_SECRET".into(), s.clone());
    }
    // 浏览器连接:部署目录自带 browserskill/bsk.exe 时按相对路径自动注入,
    // 不写绝对路径,盘符随包移动无关;extra_env 里的同名变量优先生效,
    // 因此手动在设置里配置同名变量仍可覆盖(设为空值可关闭此功能)。
    let bsk_dir = deploy_dir.join("browserskill");
    if !env.contains_key("BROWSERSKILL_BINARY") && bsk_dir.join("bsk.exe").is_file() {
        env.insert(
            "BROWSERSKILL_BINARY".into(),
            bsk_dir.join("bsk.exe").display().to_string(),
        );
        if let Ok(entries) = std::fs::read_dir(&bsk_dir) {
            let zip = entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.starts_with("browser-skill-weknora-") && n.ends_with(".zip"))
                .min();
            if let Some(zip) = zip {
                env.insert(
                    "BROWSERSKILL_EXTENSION_PATH".into(),
                    bsk_dir.join(zip).display().to_string(),
                );
            }
        }
    }
    // 桌面单机默认放行 198.18.0.0/15(Fake-IP)网段:Clash/mihomo/Surge 类
    // 代理的 Fake-IP 模式会把本机所有 DNS 解析答成该段,严格 SSRF 校验会让
    // 开代理的用户保存任何云端模型、解析任何网页全部失败,且小白无从排查。
    // 该段不可路由、无 SSRF 可触达的基础设施,localhost/内网/云元数据仍然
    // 全部拦截。extra_env 同名变量仍可覆盖(设为 false 可恢复严格校验)。
    // docreader 侧由 docreader_env 同步注入。
    if !env.contains_key("SSRF_ALLOW_FAKEIP") {
        env.insert("SSRF_ALLOW_FAKEIP".into(), "true".into());
    }
    for (k, v) in &cfg.extra_env {
        env.insert(k.clone(), v.clone());
    }
    env
}

/// docreader(解析服务)需要与 Lite 一致的 SSRF 白名单口径:网页解析的 SSRF
/// 守卫跑在 Python 里,只认这几个环境变量。历史版本没有注入,导致用户在
/// 设置里配置的 SSRF_WHITELIST_EXTRA 对解析不生效(网页导入被拦而模型可用)。
/// 只透传这几个变量,避免把 Lite 的 DB_PATH/SERVER_PORT 等环境带进 Python。
fn docreader_env(cfg: &Config, deploy_dir: &PathBuf) -> BTreeMap<String, String> {
    let full = build_env(cfg, deploy_dir);
    let mut filtered = BTreeMap::new();
    for key in [
        "SSRF_WHITELIST",
        "SSRF_WHITELIST_EXTRA",
        "SSRF_ALLOW_FAKEIP",
        "SSRF_DNS_WHITELIST_ONLY",
    ] {
        if let Some(v) = full.get(key) {
            filtered.insert(key.to_string(), v.clone());
        }
    }
    filtered
}

pub fn health_probe(base_url: &str) -> Result<u128, String> {
    let url = format!("{base_url}/health");
    let start = std::time::Instant::now();
    let resp = ureq::get(&url)
        .timeout(HEALTH_TIMEOUT)
        .call()
        .map_err(|e| format!("{e}"))?;
    let latency = start.elapsed().as_millis();
    if resp.status() == 200 {
        Ok(latency)
    } else {
        Err(format!("HTTP {}", resp.status()))
    }
}

fn append_log_file(deploy_dir: &PathBuf) -> Option<std::fs::File> {
    let dir = deploy_dir.join("data").join("logs");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("lite-{}.log", chrono_today()));
    OpenOptions::new().create(true).append(true).open(path).ok()
}

fn chrono_today() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let days = secs / 86400;
    // days since 1970-01-01 -> civil date (Howard Hinnant's algorithm)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// 启动 docreader 文档解析服务(如启用且未在运行)。
///
/// 目录由 resolve_docreader_dir 解析,Python 解释器按布局选择:
///   便携布局(assemble-docreader.sh 产出): dir/python/python.exe,
///     依赖装在其 Lib/site-packages(site.py 自动发现),包经 cwd/-m 解析;
///   开发布局(仓库 uv sync):                dir/.venv/Scripts/python.exe,
///     PYTHONPATH 指向包父目录。
fn ensure_docreader(app: &AppHandle, deploy_dir: &PathBuf) {
    let state = app.state::<ServiceState>();
    let cfg = state.config();
    if !cfg.docreader_enabled {
        return;
    }
    if std::net::TcpStream::connect("127.0.0.1:50051").is_ok() {
        return;
    }
    let Some(dir) = resolve_docreader_dir(&cfg, deploy_dir) else {
        let entry = state.push_log(
            "sys",
            "docreader 未就绪(部署目录无 docreader/,或配置的目录缺少 main.py),PDF 等复杂格式解析不可用".into(),
        );
        let _ = app.emit(LOG_EVENT, &entry);
        return;
    };
    // 便携布局优先 bundled python;两种布局都兼容另一侧的解释器位置
    let portable_layout = dir.join("docreader").join("main.py").is_file();
    let py_candidates: [PathBuf; 2] = if portable_layout {
        [dir.join("python").join("python.exe"), dir.join(".venv").join("Scripts").join("python.exe")]
    } else {
        [dir.join(".venv").join("Scripts").join("python.exe"), dir.join("python").join("python.exe")]
    };
    let Some(python) = py_candidates.into_iter().find(|p| p.is_file()) else {
        let hint = if portable_layout {
            format!(
                "docreader 便携运行时损坏(缺少 {}),请重新执行 scripts/assemble-docreader.sh",
                dir.join("python").join("python.exe").display()
            )
        } else {
            format!(
                "docreader 未安装依赖(缺少 {}),请先在该目录执行 uv sync",
                dir.join(".venv").join("Scripts").join("python.exe").display()
            )
        };
        let entry = state.push_log("sys", hint);
        let _ = app.emit(LOG_EVENT, &entry);
        return;
    };
    // 包名即目录名 docreader,PYTHONPATH 须指向其父目录:便携布局是 dir 本身
    // (dir/docreader/main.py),开发布局是 dir 的父目录(目录自身即包)。
    // 便携 python 无 ._pth,PYTHONPATH 常规生效;`-m` 同时把 cwd 计入
    // sys.path,双保险。只裸起 python.exe 会进入 REPL,读到关闭的 stdin 后
    // 立即退出,50051 永不监听(表现为流水线 DOCREADER_UNAVAILABLE)。
    let pythonpath = if portable_layout {
        dir.display().to_string()
    } else {
        dir.parent().unwrap_or(&dir).display().to_string()
    };
    let image_tmp = deploy_dir.join("data").join("docreader-tmp");
    let _ = std::fs::create_dir_all(&image_tmp);
    let mut cmd = Command::new(&python);
    cmd.current_dir(&dir)
        .args(["-m", "docreader.main"])
        .env("PYTHONPATH", &pythonpath)
        // 管道输出在中文 Windows 默认按 GBK 编码,解析器日志含中文时
        // 会 UnicodeEncodeError 噪声;强制子进程用 UTF-8 写日志。
        .env("PYTHONUTF8", "1")
        .env("LOG_LEVEL", cfg.log_level.clone())
        // 默认 /tmp/docreader 随盘符解析到 <盘>:\tmp;指到部署数据目录,
        // 免去对 U 盘根目录的写入
        .env("DOCREADER_IMAGE_OUTPUT_DIR", image_tmp.display().to_string());
    // SSRF 白名单口径与 Lite 保持一致(网页解析的守卫在 Python 里)
    for (k, v) in docreader_env(&cfg, deploy_dir) {
        cmd.env(k, v);
    }
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    match cmd.spawn() {
        Ok(mut child) => {
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();
            *state.docreader.lock() = Some(child);
            let entry = state.push_log("sys", "docreader 解析服务已启动 (127.0.0.1:50051)".into());
            let _ = app.emit(LOG_EVENT, &entry);
            if let Some(r) = stdout {
                pump(app.clone(), "docreader/stdout", r);
            }
            if let Some(r) = stderr {
                pump(app.clone(), "docreader/stderr", r);
            }
            // 就绪探测:进程拉起 ≠ 服务可用(依赖缺失/导入失败时它几秒内就退出)。
            // U 盘上冷加载 pandas/grpcio 等重依赖可能要几十秒,窗口放宽到 60s;
            // 探测结果写日志,失败时指引去看 docreader/stderr。
            {
                let app2 = app.clone();
                std::thread::spawn(move || {
                    for _ in 0..120 {
                        if std::net::TcpStream::connect("127.0.0.1:50051").is_ok() {
                            let entry = app2
                                .state::<ServiceState>()
                                .push_log("sys", "docreader 端口 50051 就绪".into());
                            let _ = app2.emit(LOG_EVENT, &entry);
                            return;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                    let entry = app2.state::<ServiceState>().push_log(
                        "sys",
                        "docreader 60s 内未监听 50051,解析将不可用;请查看 docreader/stderr 日志".into(),
                    );
                    let _ = app2.emit(LOG_EVENT, &entry);
                });
            }
        }
        Err(e) => {
            let entry = state.push_log("sys", format!("docreader 启动失败: {e}"));
            let _ = app.emit(LOG_EVENT, &entry);
        }
    }
}

fn stop_docreader(app: &AppHandle) {
    let state = app.state::<ServiceState>();
    let mut guard = state.docreader.lock();
    if let Some(mut c) = guard.take() {
        let _ = c.kill();
        let _ = c.wait();
        let entry = state.push_log("sys", "docreader 解析服务已停止".into());
        let _ = app.emit(LOG_EVENT, &entry);
    }
}

/// 拉起 Lite 服务并启动监控线程(日志采集 + 健康探测)
pub fn start(app: &AppHandle) -> Result<StatusInfo, String> {
    let state = app.state::<ServiceState>();
    // 串行化:并发触发(自动启动线程 / 托盘 / 控制台按钮)必须排队,
    // 后到者在锁释放后走下方状态检查,直接得到"服务已在启动/运行状态"。
    let _start_guard = state.start_lock.lock();
    {
        let st = *state.status.lock();
        if st == Status::Starting || st == Status::Running {
            return Err(format!("服务已在{}状态", st.as_str()));
        }
    }
    let mut cfg = state.config();
    cfg.sanitize();
    let dir = resolve_deploy(app, &cfg).ok_or_else(|| {
        "未找到 LinkMind 后端程序。请在设置中指定后端可执行文件路径,或将部署目录放入 runtime/。".to_string()
    })?;
    let exe = lite_exe_path(&dir).ok_or_else(|| {
        "部署目录中未找到后端程序(linkmind-lite.exe)。".to_string()
    })?;
    if !dir.join("config").join("config.yaml").is_file() && !dir.join("config.yaml").is_file() {
        return Err("部署目录缺少 config/config.yaml,Lite 服务无法启动".into());
    }

    // WeKnora 后端把 BrowserSkill 守护进程的临时目录固定为所在盘根目录的
    // \tmp(Go 侧 os.MkdirTemp("/tmp", ...),随进程工作目录的盘符解析),
    // 缺失时配对会报"守护进程启动失败"。便携包可能落在任意盘符,
    // 按部署目录所在盘自动补建,失败仅记录日志,不阻断启动。
    let dir_str = dir.display().to_string();
    let bytes = dir_str.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        let tmp_dir = format!("{}:\\tmp", bytes[0] as char);
        if let Err(e) = std::fs::create_dir_all(&tmp_dir) {
            let entry = state.push_log(
                "sys",
                format!("无法创建 {tmp_dir}(浏览器守护进程临时目录): {e}"),
            );
            let _ = app.emit(LOG_EVENT, &entry);
        }
    }

    // 首次启动自动生成密钥并持久化
    if cfg.system_aes_key.is_none() || cfg.jwt_secret.is_none() {
        if cfg.system_aes_key.is_none() {
            cfg.system_aes_key = Some(config::random_hex());
        }
        if cfg.jwt_secret.is_none() {
            cfg.jwt_secret = Some(config::random_hex());
        }
        state.set_config(cfg.clone());
        config::save(app, &cfg).ok();
    }

    *state.last_error.lock() = None;
    *state.health.lock() = (None, None);
    *state.log_file.lock() = append_log_file(&dir);
    // 提前置为 Starting:taskkill/固定休眠/拉起子进程都算启动中,
    // 避免这段窗口内并发 start 再次穿过检查(旧实现在 spawn 后才翻转)。
    *state.status.lock() = Status::Starting;
    emit_status(app);

    // 清理残留的 Lite 进程(如上次壳被强杀留下的孤儿),避免端口占用导致启动失败。
    // 新旧二进制名都要清(旧包升级场景可能两者并存)。
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW_TASKKILL: u32 = 0x0800_0000;
        for image in ["linkmind-lite.exe", "weknora-lite.exe"] {
            if let Ok(out) = Command::new("taskkill")
                .args(["/IM", image, "/F"])
                .creation_flags(CREATE_NO_WINDOW_TASKKILL)
                .output()
            {
                if out.status.success() {
                    // taskkill 输出是系统 OEM 编码(中文系统为 GBK),按 UTF-8 解读会乱码;
                    // 日志只保留 ASCII 数字形式的 PID(GBK 多字节序列不含数字区,数字不受影响)
                    let pid = String::from_utf8_lossy(&out.stdout)
                        .split(|c: char| !c.is_ascii_digit())
                        .filter(|s| !s.is_empty())
                        .last()
                        .unwrap_or("")
                        .to_string();
                    let detail = if pid.is_empty() {
                        format!("检测到残留的后端进程({image}),已结束")
                    } else {
                        format!("检测到残留的后端进程({image}),已结束 (PID {pid})")
                    };
                    let entry = state.push_log("sys", detail);
                    let _ = app.emit(LOG_EVENT, &entry);
                }
            }
        }
    }

    ensure_docreader(app, &dir);
    std::thread::sleep(std::time::Duration::from_millis(1200));

    let envs = build_env(&cfg, &dir);
    // 继承当前进程环境(Windows 下 Winsock 初始化依赖 SystemRoot 等系统变量,
    // 绝不能 env_clear),再用服务专用变量叠加/覆盖。
    let mut cmd = Command::new(&exe);
    cmd.current_dir(&dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in &envs {
        cmd.env(k, v);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            *state.status.lock() = Status::Failed;
            *state.last_error.lock() = Some(format!("启动 LinkMind 后端失败: {e}"));
            let entry = state.push_log("sys", format!("启动 LinkMind 后端失败: {e}"));
            let _ = app.emit(LOG_EVENT, &entry);
            emit_status(app);
            return Err(format!("启动 LinkMind 后端失败: {e}"));
        }
    };

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let pid = child.id();
    *state.child.lock() = Some(child);
    *state.started_at.lock() = Some(SystemTime::now());
    *state.status.lock() = Status::Starting;

    let gen = {
        let mut g = state.generation.lock();
        *g += 1;
        *g
    };

    {
        let entry = state.push_log("sys", format!("服务进程已启动 (pid {pid}), 工作目录: {}", dir.display()));
        let _ = app.emit(LOG_EVENT, &entry);
    }
    emit_status(app);

    // stdout / stderr 采集线程
    pump(app.clone(), "stdout", stdout);
    pump(app.clone(), "stderr", stderr);

    // 监控线程: 健康探测直到 Running,然后等待进程退出
    {
        let app2 = app.clone();
        let base_url = cfg.base_url();
        std::thread::spawn(move || {
            let state = app2.state::<ServiceState>();
            let deadline = std::time::Instant::now() + STARTUP_HEALTH_WINDOW;
            loop {
                if *state.generation.lock() != gen {
                    return; // 已被新一轮 start/stop 取代
                }
                std::thread::sleep(Duration::from_millis(500));
                if std::time::Instant::now() > deadline {
                    *state.status.lock() = Status::Failed;
                    *state.last_error.lock() = Some("健康检查超时:服务在 180s 内未就绪".into());
                    state.push_log("sys", "健康检查超时".into());
                    emit_status(&app2);
                    return;
                }
                match health_probe(&base_url) {
                    Ok(latency) => {
                        *state.health.lock() = (Some(true), Some(latency));
                        if *state.status.lock() == Status::Starting {
                            *state.status.lock() = Status::Running;
                            let entry = state.push_log(
                                "sys",
                                format!("健康检查通过 ({latency}ms),服务就绪: {base_url}"),
                            );
                            let _ = app2.emit(LOG_EVENT, &entry);
                            emit_status(&app2);
                        }
                        break;
                    }
                    Err(e) => {
                        *state.health.lock() = (Some(false), None);
                        if *state.generation.lock() != gen {
                            return;
                        }
                        let exited = state
                            .child
                            .lock()
                            .as_mut()
                            .map(|c| c.try_wait().map(|o| o.is_some()).unwrap_or(false))
                            .unwrap_or(true);
                        if exited {
                            let code = state
                                .child
                                .lock()
                                .as_mut()
                                .and_then(|c| c.wait().ok())
                                .and_then(|s| s.code());
                            *state.status.lock() = Status::Failed;
                            *state.last_error.lock() =
                                Some(format!("进程提前退出(退出码 {code:?}): {e}"));
                            let entry = state.push_log("sys", format!("进程提前退出(退出码 {code:?})"));
                            let _ = app2.emit(LOG_EVENT, &entry);
                            *state.child.lock() = None;
                            emit_status(&app2);
                            return;
                        }
                    }
                }
            }
            // 进入 Running 后继续守护,捕获进程退出
            loop {
                if *state.generation.lock() != gen {
                    return;
                }
                std::thread::sleep(Duration::from_millis(800));
                let mut child = state.child.lock();
                if let Some(c) = child.as_mut() {
                    match c.try_wait() {
                        Ok(Some(s)) => {
                            let code = s.code();
                            drop(child);
                            let was_running = *state.status.lock() == Status::Running;
                            if *state.generation.lock() != gen {
                                return;
                            }
                            *state.child.lock() = None;
                            *state.health.lock() = (None, None);
                            if was_running {
                                *state.status.lock() = Status::Stopped;
                                *state.last_error.lock() = Some("服务进程意外退出".into());
                                let entry = state.push_log("sys", format!("服务进程退出(退出码 {code:?})"));
                                let _ = app2.emit(LOG_EVENT, &entry);
                            }
                            emit_status(&app2);
                            return;
                        }
                        Ok(None) => {}
                        Err(_) => return,
                    }
                } else {
                    return;
                }
                // 运行中周期性刷新健康延迟
                if *state.status.lock() == Status::Running {
                    if let Ok(latency) = health_probe(&base_url) {
                        *state.health.lock() = (Some(true), Some(latency));
                    } else {
                        *state.health.lock() = (Some(false), None);
                    }
                }
            }
        });
    }

    Ok(state.snapshot())
}

pub fn stop(app: &AppHandle) -> Result<StatusInfo, String> {
    let state = app.state::<ServiceState>();
    let had_child = {
        let mut guard = state.child.lock();
        let had = guard.is_some();
        if had {
            *state.status.lock() = Status::Stopping;
        }
        had
    };
    if had_child {
        emit_status(app);
    }
    {
        let mut guard = state.child.lock();
        if let Some(c) = guard.as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
        *guard = None;
    }
    *state.status.lock() = Status::Stopped;
    *state.health.lock() = (None, None);
    *state.started_at.lock() = None;
    stop_docreader(app);
    {
        let entry = state.push_log("sys", "服务已停止".into());
        let _ = app.emit(LOG_EVENT, &entry);
    }
    emit_status(app);
    Ok(state.snapshot())
}

pub fn restart(app: &AppHandle) -> Result<StatusInfo, String> {
    stop(app)?;
    std::thread::sleep(Duration::from_millis(600));
    start(app)
}

/// 应用退出时清理服务进程(除非配置了 keep_after_exit)
pub fn shutdown_cleanup(app: &AppHandle) {
    let state = app.state::<ServiceState>();
    let keep = state.config().keep_after_exit;
    if keep {
        return;
    }
    // 与 start() 互斥:退出若撞上启动准备期,等它把子进程拉起后再一并结束,
    // 否则刚 spawn 的进程无人跟踪,沦为孤儿服务。
    let _start_guard = state.start_lock.lock();
    let mut guard = state.child.lock();
    if let Some(c) = guard.as_mut() {
        let _ = c.kill();
        let _ = c.wait();
    }
    *guard = None;
    drop(guard);
    stop_docreader(app);
}

/// WebView2 偶发"新窗口内容已渲染但不上屏"的合成问题;
/// 做一次尺寸抖动强制重绘,并聚焦窗口。
fn nudge_composition(win: &tauri::WebviewWindow) {
    let w = win.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        if let Ok(size) = w.inner_size() {
            let (wd, ht) = (f64::from(size.width), f64::from(size.height));
            use tauri::PhysicalSize;
            let _ = w.set_size(PhysicalSize::new(wd as u32 + 1, ht as u32 + 1));
            std::thread::sleep(std::time::Duration::from_millis(120));
            let _ = w.set_size(PhysicalSize::new(wd as u32, ht as u32));
        }
        let _ = w.set_focus();
    });
}

/// 在独立 Tauri 窗口中打开官方 WeKnora Web 界面(顶层加载,不受 X-Frame-Options 限制)
pub fn open_workbench(app: &AppHandle) -> Result<String, String> {
    let mut cfg = app.state::<ServiceState>().config();
    cfg.sanitize();
    if let Err(e) = health_probe(&cfg.base_url()) {
        return Err(format!(
            "服务未就绪({e})。请先在控制台启动服务,待状态变为「运行中」后再打开工作台。"
        ));
    }
    let base = cfg.base_url();
    let state = app.state::<ServiceState>();

    // 工作台窗口在应用启动时预建(隐藏)。若被外部关闭则重建。
    let win = match app.get_webview_window("workbench") {
        Some(w) => w,
        None => create_workbench_window(app)?,
    };

    let _ = win.show();
    let _ = win.unminimize();
    let _ = win.set_focus();
    let _ = win.navigate(
        format!("{base}/").parse().map_err(|e| format!("URL 解析失败: {e}"))?,
    );

    // 导航校验循环: 不信任 navigate() 的返回值(Tauri 在 Windows 上存在
    // 返回 Ok 但导航被丢弃的缺陷),直接检查实际 URL,失败则重试/JS 兜底。
    {
        let w2 = win.clone();
        let app2 = app.clone();
        std::thread::spawn(move || {
            let expected = format!("{}/", base_url_for(&app2));
            for attempt in 0..8 {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let current = w2
                    .url()
                    .map(|u| u.to_string())
                    .unwrap_or_else(|_| "about:blank".into());
                if current.starts_with(&expected) {
                    if attempt > 0 {
                        let entry = w2.state::<ServiceState>().push_log(
                            "sys",
                            format!("工作台导航成功(第 {} 次重试)", attempt),
                        );
                        let _ = app2.emit(LOG_EVENT, &entry);
                    }
                    nudge_composition(&w2);
                    return;
                }
                let url: tauri::Url = match expected.parse() {
                    Ok(u) => u,
                    Err(_) => return,
                };
                if w2.navigate(url).is_err() && attempt >= 4 {
                    let _ = w2.eval(&format!("location.replace('{}')", expected));
                }
            }
            let entry = w2
                .state::<ServiceState>()
                .push_log("sys", "工作台导航重试超时,请关闭工作台窗口后重试".into());
            let _ = app2.emit(LOG_EVENT, &entry);
        });
    }

    let entry = state.push_log("sys", "工作台窗口已打开,正在加载界面…".into());
    let _ = app.emit(LOG_EVENT, &entry);
    Ok("shown".into())
}

/// 工作台内的新窗口请求(target="_blank" 链接 / window.open)。wry 未注册处理器时
/// 会直接取消该请求,表现为界面里所有新标签链接(如聊天引用的「在图谱中查看」、
/// 文档/GitHub 外链)点击无反应。这里接管:同源链接在工作台窗口内跳转,
/// 外部链接交给系统默认浏览器。
fn handle_workbench_new_window(
    app: &AppHandle,
    url: tauri::Url,
) -> tauri::webview::NewWindowResponse<tauri::Wry> {
    let cfg = app.state::<ServiceState>().config();
    let same_origin = url.to_string().starts_with(&cfg.base_url())
        || (matches!(
            url.host_str(),
            Some("127.0.0.1") | Some("localhost") | Some("[::1]")
        ) && url.port() == Some(cfg.port));

    if same_origin {
        if let Some(win) = app.get_webview_window("workbench") {
            let _ = win.navigate(url);
        }
    } else if matches!(url.scheme(), "http" | "https") {
        let _ = app.opener().open_url(url.to_string(), None::<&str>);
    }
    tauri::webview::NewWindowResponse::Deny
}

/// 预建隐藏的工作台窗口(应用启动时调用);URL 稍后由 open_workbench 导航。
pub fn create_workbench_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    let new_window_app = app.clone();
    tauri::webview::WebviewWindowBuilder::new(
        app,
        "workbench",
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title("LinkMind 工作台")
    .inner_size(1360.0, 880.0)
    .min_inner_size(960.0, 640.0)
    .visible(false)
    .on_new_window(move |url, _features| handle_workbench_new_window(&new_window_app, url))
    .build()
    .map_err(|e| format!("创建工作台窗口失败: {e}"))
}

fn base_url_for(app: &AppHandle) -> String {
    app.state::<ServiceState>().config().base_url()
}
