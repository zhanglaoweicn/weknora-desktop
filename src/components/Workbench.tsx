import { useState } from "react";
import { api, type StatusInfo } from "../api";

export default function Workbench({ status }: { status: StatusInfo | null }) {
  const running = status?.status === "running";
  const url = status?.base_url ?? "http://127.0.0.1:8080";
  const [msg, setMsg] = useState<string | null>(null);

  return (
    <div className="page">
      <div className="page-head">
        <h1>工作台</h1>
        <p>LinkMind 知识库界面(独立窗口加载,数据不出本机)</p>
      </div>

      <div className="grid-2">
        <section className="card">
          <div className="card-title">启动完整界面</div>
          {running ? (
            <>
              <p className="muted">
                服务已就绪(<span className="mono">{url}</span>)。LinkMind
                的完整界面——知识库管理、文档解析、对话问答、智能体——将在独立窗口中打开。
              </p>
              <div className="btn-row">
                <button
                  className="btn primary"
                  onClick={() =>
                    api
                      .openWorkbench()
                      .then(() => setMsg(null))
                      .catch((e) => setMsg(String(e)))
                  }
                >
                  打开工作台窗口
                </button>
                <button className="btn ghost" onClick={() => api.openUrl(url).catch(() => {})}>
                  在浏览器打开
                </button>
              </div>
              {msg && <div className="form-error">{msg}</div>}
            </>
          ) : (
            <>
              <p className="muted">
                服务尚未运行。请先在「控制台」启动 LinkMind Lite 服务,再打开工作台。
              </p>
              <div className="status-row" style={{ marginTop: 8 }}>
                <span className="status-dot idle" />
                <span className="status-hint">
                  {status?.last_error ?? "等待服务启动…"}
                </span>
              </div>
            </>
          )}
        </section>

        <section className="card">
          <div className="card-title">直连 API</div>
          <p className="muted">LinkMind 提供 REST API 与 MCP Server,可从任意客户端调用:</p>
          <div className="kv">
            <div className="kv-row">
              <span className="kv-k">REST API</span>
              <span className="kv-v mono">{url}/api/v1</span>
            </div>
            <div className="kv-row">
              <span className="kv-k">健康检查</span>
              <span className="kv-v mono">{url}/health</span>
            </div>
            <div className="kv-row">
              <span className="kv-k">API 文档</span>
              <span className="kv-v mono">{url}/swagger/index.html</span>
            </div>
          </div>
          <p className="muted small">
            认证方式:JWT 或空间级 API Key(请求头 X-API-Key),详见官方文档。
          </p>
        </section>
      </div>
    </div>
  );
}
