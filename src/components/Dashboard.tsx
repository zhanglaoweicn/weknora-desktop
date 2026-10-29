import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  events,
  fmtUptime,
  inTauri,
  type LogLine,
  type StatusInfo,
} from "../api";
import LogsPanel from "./LogsPanel";

export default function Dashboard({
  status,
  onRefresh,
}: {
  status: StatusInfo | null;
  onRefresh: () => void;
}) {
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [logs, setLogs] = useState<LogLine[]>([]);
  const logsRef = useRef<LogLine[]>([]);
  logsRef.current = logs;

  useEffect(() => {
    api.getLogs(800).then(setLogs).catch(() => {});
    const un = events.onLog((line) => {
      setLogs((prev) => {
        const next = [...prev, line];
        return next.length > 3000 ? next.slice(next.length - 3000) : next;
      });
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  const act = useCallback(
    async (name: string, fn: () => Promise<unknown>) => {
      setBusy(name);
      setError(null);
      try {
        await fn();
        onRefresh();
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(null);
      }
    },
    [onRefresh],
  );

  const st = status;
  const running = st?.status === "running";
  const starting = st?.status === "starting" || st?.status === "stopping";
  const failed = st?.status === "failed";

  return (
    <div className="page">
      <div className="page-head">
        <h1>控制台</h1>
        <p>LinkMind Lite 服务生命周期与运行状态</p>
      </div>

      <div className="grid-2">
        <section className="card">
          <div className="card-title">服务状态</div>
          <div className="status-row">
            <span
              className={
                "status-dot " +
                (running ? "ok" : failed ? "err" : starting ? "warn" : "idle")
              }
            />
            <div>
              <div className="status-big">{st?.status_text ?? "—"}</div>
              {st?.last_error && <div className="status-err">{st.last_error}</div>}
              {!st?.last_error && (
                <div className="status-hint">
                  {running
                    ? `健康检查通过 · ${st.health_latency_ms ?? "?"} ms`
                    : starting
                      ? "正在等待服务就绪…"
                      : "服务未运行"}
                </div>
              )}
            </div>
          </div>
          <div className="kv">
            <div className="kv-row">
              <span className="kv-k">服务地址</span>
              <span className="kv-v mono">{st?.base_url ?? "—"}</span>
            </div>
            <div className="kv-row">
              <span className="kv-k">进程 PID</span>
              <span className="kv-v mono">{st?.pid ?? "—"}</span>
            </div>
            <div className="kv-row">
              <span className="kv-k">运行时长</span>
              <span className="kv-v">{fmtUptime(st?.uptime_secs ?? null)}</span>
            </div>
          </div>
          <div className="btn-row">
            <button
              className="btn primary"
              disabled={!!busy || running || starting}
              onClick={() => act("start", api.startService)}
            >
              {busy === "start" ? "启动中…" : "启动服务"}
            </button>
            <button
              className="btn danger-ghost"
              disabled={!!busy || (!running && !starting)}
              onClick={() => act("stop", api.stopService)}
            >
              {busy === "stop" ? "停止中…" : "停止服务"}
            </button>
            <button
              className="btn ghost"
              disabled={!!busy || (!running && !starting)}
              onClick={() => act("restart", api.restartService)}
            >
              {busy === "restart" ? "重启中…" : "重启"}
            </button>
          </div>
          {error && <div className="form-error">{error}</div>}
        </section>

        <section className="card">
          <div className="card-title">快速入口</div>
          <button
            className="hero-btn"
            disabled={!running}
            title={running ? "" : "服务运行后可用"}
            onClick={() => api.openWorkbench().catch((e) => setError(String(e)))}
          >
            <span className="hero-icon">◈</span>
            <span>
              <span className="hero-label">打开 LinkMind 工作台</span>
              <span className="hero-desc">在独立窗口中使用完整知识库界面</span>
            </span>
          </button>
          <div className="btn-row" style={{ marginTop: 12 }}>
            <button
              className="btn ghost"
              disabled={!running}
              onClick={() =>
                api.openUrl(st?.base_url ?? "http://127.0.0.1:8080").catch(() => {})
              }
            >
              在浏览器打开
            </button>
            <button className="btn ghost" onClick={() => api.revealDataDir().catch(() => {})}>
              打开数据目录
            </button>
          </div>
          <div className="tips">
            <div className="tips-title">使用提示</div>
            <ul>
              <li>首次进入工作台后,按页面引导配置模型(推荐本地 Ollama 或任意 OpenAI 兼容服务)。</li>
              <li>数据全部保存在本地数据目录,可随时在设置中打开。</li>
              <li>Lite 版为单机模式:SQLite 存储 + 内置检索,无外部依赖。</li>
            </ul>
          </div>
          {!inTauri && (
            <div className="tips warn">
              当前为浏览器预览,服务控制按钮不会生效。
            </div>
          )}
        </section>
      </div>

      <LogsPanel
        logs={logs}
        onClear={() => {
          api.clearLogs().catch(() => {});
          setLogs([]);
        }}
      />
    </div>
  );
}
