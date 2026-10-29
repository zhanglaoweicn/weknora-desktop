import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** 浏览器预览模式下 invoke 不可用,统一降级处理 */
export const inTauri = "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>, fallback?: T): Promise<T> {
  if (!inTauri) {
    if (fallback !== undefined) return fallback;
    throw new Error("浏览器预览模式:Tauri 命令不可用");
  }
  return invoke<T>(cmd, args);
}

export type ServiceStatus = "stopped" | "starting" | "running" | "stopping" | "failed";

export interface StatusInfo {
  status: ServiceStatus;
  status_text: string;
  pid: number | null;
  host: string;
  port: number;
  base_url: string;
  started_at: number | null;
  uptime_secs: number | null;
  health_ok: boolean | null;
  health_latency_ms: number | null;
  last_error: string | null;
}

export interface LogLine {
  ts: number;
  stream: string;
  line: string;
}

export interface AppConfig {
  host: string;
  port: number;
  auto_start: boolean;
  keep_after_exit: boolean;
  lite_path: string | null;
  deploy_dir: string | null;
  ollama_base_url: string;
  log_level: string;
  extra_env: Record<string, string>;
  docreader_enabled: boolean;
  docreader_dir: string | null;
}

export interface DeployInfo {
  lite_path: string | null;
  lite_found: boolean;
  deploy_dir: string | null;
  web_ready: boolean;
  config_ready: boolean;
  migrations_ready: boolean;
  data_dir: string | null;
  docreader_ready: boolean;
}

export interface HealthInfo {
  ok: boolean;
  latency_ms: number | null;
  error: string | null;
}

export const api = {
  getConfig: () => call<AppConfig>("get_config", undefined, {
    host: "127.0.0.1", port: 8080, auto_start: true, keep_after_exit: false,
    lite_path: null, deploy_dir: null, ollama_base_url: "http://127.0.0.1:11434",
    log_level: "info", extra_env: {}, docreader_enabled: true, docreader_dir: null,
  }),
  saveConfig: (cfg: AppConfig) => call<AppConfig>("save_config", { cfg }),
  getStatus: () => call<StatusInfo>("get_status", undefined, {
    status: "stopped", status_text: "已停止", pid: null, host: "127.0.0.1", port: 8080,
    base_url: "http://127.0.0.1:8080", started_at: null, uptime_secs: null,
    health_ok: null, health_latency_ms: null, last_error: null,
  }),
  getLogs: (limit = 500) => call<LogLine[]>("get_logs", { limit }, []),
  clearLogs: () => call<void>("clear_logs"),
  startService: () => call<StatusInfo>("start_service"),
  stopService: () => call<StatusInfo>("stop_service"),
  restartService: () => call<StatusInfo>("restart_service"),
  healthCheck: () => call<HealthInfo>("health_check", undefined, { ok: false, latency_ms: null, error: "浏览器预览" }),
  deployInfo: () => call<DeployInfo>("deploy_info", undefined, {
    lite_path: null, lite_found: false, deploy_dir: null,
    web_ready: false, config_ready: false, migrations_ready: false, data_dir: null,
    docreader_ready: false,
  }),
  openWorkbench: () => call<string>("open_workbench"),
  openUrl: (url: string) => call<void>("open_url", { url }),
  openPath: (path: string) => call<void>("open_path", { path }),
  revealDataDir: () => call<void>("reveal_data_dir"),
};

export const events = {
  onLog: (cb: (line: LogLine) => void) =>
    listen<LogLine>("service://log", (e) => cb(e.payload)),
  onStatus: (cb: (info: StatusInfo) => void) =>
    listen<StatusInfo>("service://status", (e) => cb(e.payload)),
};

export function fmtUptime(secs: number | null): string {
  if (secs == null) return "—";
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  if (h > 0) return `${h} 时 ${m} 分`;
  if (m > 0) return `${m} 分 ${s} 秒`;
  return `${s} 秒`;
}

export function fmtTime(ts: number): string {
  const d = new Date(ts);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}
