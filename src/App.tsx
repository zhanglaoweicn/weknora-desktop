import { useEffect, useState } from "react";
import brandIcon from "../icon-src.png";
import { api, events, type StatusInfo } from "./api";
import Dashboard from "./components/Dashboard";
import Workbench from "./components/Workbench";
import SettingsPage from "./components/SettingsPage";

type Page = "dashboard" | "workbench" | "settings";

const NAV: { id: Page; label: string; icon: string; desc: string }[] = [
  { id: "dashboard", label: "控制台", icon: "▤", desc: "服务状态与日志" },
  { id: "workbench", label: "工作台", icon: "◈", desc: "打开 LinkMind 界面" },
  { id: "settings", label: "设置", icon: "⚙", desc: "端口、模型与部署" },
];

export default function App() {
  const [page, setPage] = useState<Page>("dashboard");
  const [status, setStatus] = useState<StatusInfo | null>(null);
  const [previewMode, setPreviewMode] = useState(false);

  useEffect(() => {
    api.getStatus().then(setStatus).catch(() => setPreviewMode(true));
    const un1 = events.onStatus(setStatus);
    return () => {
      un1.then((f) => f());
    };
  }, []);

  const dot =
    status?.status === "running"
      ? "ok"
      : status?.status === "starting" || status?.status === "stopping"
        ? "warn"
        : status?.status === "failed"
          ? "err"
          : "idle";

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand">
          <img src={brandIcon} className="brand-mark" alt="LinkMind" />
          <div>
            <div className="brand-name">LinkMind Desktop</div>
            <div className="brand-sub">本地知识库控制中心</div>
          </div>
        </div>
        <nav>
          {NAV.map((n) => (
            <button
              key={n.id}
              className={"nav-item" + (page === n.id ? " active" : "")}
              onClick={() => setPage(n.id)}
            >
              <span className="nav-icon">{n.icon}</span>
              <span>
                <span className="nav-label">{n.label}</span>
                <span className="nav-desc">{n.desc}</span>
              </span>
            </button>
          ))}
        </nav>
        <div className="sidebar-footer">
          <span className={"dot " + dot} />
          <span>服务{status?.status_text ?? "状态未知"}</span>
        </div>
      </aside>
      <main className="content">
        {previewMode && (
          <div className="banner">浏览器预览模式:服务控制功能需在 Tauri 应用内使用</div>
        )}
        {page === "dashboard" && <Dashboard status={status} onRefresh={() => api.getStatus().then(setStatus).catch(() => {})} />}
        {page === "workbench" && <Workbench status={status} />}
        {page === "settings" && <SettingsPage />}
      </main>
    </div>
  );
}
