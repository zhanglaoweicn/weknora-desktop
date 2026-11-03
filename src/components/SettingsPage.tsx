import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { api, events, type AppConfig, type DeployInfo, type UpdateMeta, type UpdateProgress } from "../api";

export default function SettingsPage() {
  const [cfg, setCfg] = useState<AppConfig | null>(null);
  const [deploy, setDeploy] = useState<DeployInfo | null>(null);
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [envText, setEnvText] = useState("");
  const [appVersion, setAppVersion] = useState("");
  const [update, setUpdate] = useState<UpdateMeta | null>(null);
  const [checking, setChecking] = useState(false);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);

  useEffect(() => {
    api.getConfig().then((c) => {
      setCfg(c);
      setEnvText(
        Object.entries(c.extra_env ?? {})
          .map(([k, v]) => `${k}=${v}`)
          .join("\n"),
      );
    });
    api.deployInfo().then(setDeploy).catch(() => {});
    getVersion().then(setAppVersion).catch(() => {});
    const un = events.onUpdateProgress((p) => {
      setProgress(p);
      if (p.stage === "failed") {
        setInstalling(false);
        setMsg({ ok: false, text: "自动更新失败,请查看服务日志后重试,或到发布页手动下载。" });
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  const checkUpdate = async () => {
    setChecking(true);
    setUpdateError(null);
    setUpdate(null);
    try {
      setUpdate(await api.checkUpdate());
    } catch (e) {
      setUpdateError(String(e));
    } finally {
      setChecking(false);
    }
  };

  const doInstall = async () => {
    setInstalling(true);
    setProgress({ stage: "downloading", downloaded: 0, total: null });
    try {
      await api.installUpdate();
    } catch (e) {
      setInstalling(false);
      setUpdateError(String(e));
    }
  };

  if (!cfg) return <div className="page"><p className="muted">加载配置…</p></div>;

  const set = (patch: Partial<AppConfig>) => setCfg({ ...cfg, ...patch });

  const parseEnv = (text: string): Record<string, string> => {
    const out: Record<string, string> = {};
    for (const raw of text.split(/\r?\n/)) {
      const line = raw.trim();
      if (!line || line.startsWith("#")) continue;
      const i = line.indexOf("=");
      if (i <= 0) continue;
      out[line.slice(0, i).trim()] = line.slice(i + 1).trim();
    }
    return out;
  };

  // Fake-IP 兼容默认开启(内置环境变量 SSRF_ALLOW_FAKEIP=true),只有用户
  // 在附加环境变量里显式写 false 才关闭。开关展示的就是这个覆盖项。
  const fakeipEnabled =
    (parseEnv(envText)["SSRF_ALLOW_FAKEIP"] ?? "true").trim().toLowerCase() !== "false";
  const setFakeip = (on: boolean) => {
    const entries = Object.entries(parseEnv(envText)).filter(
      ([k]) => k !== "SSRF_ALLOW_FAKEIP",
    );
    if (!on) entries.push(["SSRF_ALLOW_FAKEIP", "false"]);
    setEnvText(entries.map(([k, v]) => `${k}=${v}`).join("\n"));
  };

  const save = async () => {
    if (!cfg) return;
    setSaving(true);
    setMsg(null);
    try {
      const next = { ...cfg, extra_env: parseEnv(envText) };
      const saved = await api.saveConfig(next);
      setCfg(saved);
      setEnvText(
        Object.entries(saved.extra_env ?? {})
          .map(([k, v]) => `${k}=${v}`)
          .join("\n"),
      );
      setMsg({ ok: true, text: "已保存。端口/地址变更将在下次启动服务时生效。" });
      api.deployInfo().then(setDeploy).catch(() => {});
    } catch (e) {
      setMsg({ ok: false, text: String(e) });
    } finally {
      setSaving(false);
    }
  };

  const pickLiteExe = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({
        multiple: false,
        filters: [{ name: "后端程序", extensions: ["exe"] }],
      });
      if (typeof picked === "string") {
        set({ lite_path: picked });
      }
    } catch {
      setMsg({ ok: false, text: "文件选择器仅在 Tauri 应用内可用,请直接填写路径。" });
    }
  };

  return (
    <div className="page">
      <div className="page-head">
        <h1>设置</h1>
        <p>服务参数与部署诊断(保存后重启服务生效)</p>
      </div>

      <div className="grid-2">
        <section className="card">
          <div className="card-title">服务监听</div>
          <div className="form-grid">
            <label>
              监听地址
              <input
                value={cfg.host}
                onChange={(e) => set({ host: e.target.value })}
                placeholder="127.0.0.1"
              />
            </label>
            <label>
              端口
              <input
                type="number"
                value={cfg.port}
                onChange={(e) => set({ port: Number(e.target.value) || 8080 })}
              />
            </label>
            <label>
              日志级别
              <select
                value={cfg.log_level}
                onChange={(e) => set({ log_level: e.target.value })}
              >
                {["debug", "info", "warn", "error"].map((l) => (
                  <option key={l} value={l}>
                    {l}
                  </option>
                ))}
              </select>
            </label>
          </div>
          <label className="switch-row">
            <input
              type="checkbox"
              checked={cfg.auto_start}
              onChange={(e) => set({ auto_start: e.target.checked })}
            />
            <span>应用启动时自动启动服务</span>
          </label>
          <label className="switch-row">
            <input
              type="checkbox"
              checked={cfg.keep_after_exit}
              onChange={(e) => set({ keep_after_exit: e.target.checked })}
            />
            <span>关闭应用后保留服务进程(后台常驻)</span>
          </label>
        </section>

        <section className="card">
          <div className="card-title">模型与依赖</div>
          <label className="field">
            Ollama 服务地址
            <input
              value={cfg.ollama_base_url}
              onChange={(e) => set({ ollama_base_url: e.target.value })}
              placeholder="http://127.0.0.1:11434"
            />
            <span className="field-hint">
              本地向量/对话模型走此地址;也可在工作台中改用任意 OpenAI 兼容服务。
            </span>
          </label>
          <label className="switch-row" style={{ marginTop: 8 }}>
            <input
              type="checkbox"
              checked={fakeipEnabled}
              onChange={(e) => setFakeip(e.target.checked)}
            />
            <span>VPN/代理兼容模式（推荐开启）</span>
          </label>
          <p className="field-hint">
            开启后兼容 Clash / Verge 等代理软件的 Fake-IP
            模式。此前保存云端模型报"198.18.x.x 被拦截"、解析网页报"URL blocked by SSRF
            guard"都是这个原因；保持开启即可在开代理的情况下正常使用。
          </p>
          <label className="field">
            更新源地址(可选,留空使用默认发布源;须为 HTTPS 的 latest.json 完整地址)
            <input
              className="mono"
              value={cfg.update_url ?? ""}
              onChange={(e) => set({ update_url: e.target.value || null })}
              placeholder="https://your-server.com/linkmind/latest.json"
            />
            <span className="field-hint">
              客户访问 GitHub 缓慢时,可把它指向自建服务器/OSS 镜像,更新内容与官方发布一致。
            </span>
          </label>
          <label className="field">
            附加环境变量(每行 KEY=VALUE,覆盖内置值)
            <textarea
              rows={5}
              className="mono"
              value={envText}
              onChange={(e) => setEnvText(e.target.value)}
              placeholder={"# OPENAI_API_KEY=sk-...\n# OPENAI_BASE_URL=https://api.openai.com/v1\n# SSRF_WHITELIST_EXTRA=open.bigmodel.cn,*.example.com"}
            />
            <span className="field-hint">
              遇到"未通过安全校验 / SSRF validation
              failed"时，可把对应域名加入 SSRF_WHITELIST_EXTRA（支持 *.example.com
              通配，逗号分隔，对模型调用与网页解析同时生效）。
            </span>
          </label>
        </section>
      </div>

      <div className="grid-2">
        <section className="card">
          <div className="card-title">部署路径</div>
          <label className="field">
            后端程序路径(linkmind-lite.exe;留空使用内置解析:资源目录 → runtime/)
            <div className="input-row">
              <input
                className="mono"
                value={cfg.lite_path ?? ""}
                onChange={(e) => set({ lite_path: e.target.value || null })}
                placeholder="D:\\KnowLege\\weknora-desktop\\runtime\\weknora-lite.exe"
              />
              <button className="btn ghost" onClick={pickLiteExe}>
                浏览
              </button>
            </div>
          </label>
          <label className="field">
            部署目录(留空则跟随可执行文件所在目录)
            <input
              className="mono"
              value={cfg.deploy_dir ?? ""}
              onChange={(e) => set({ deploy_dir: e.target.value || null })}
              placeholder="自动"
            />
          </label>
          <label className="switch-row" style={{ marginTop: 12 }}>
            <input
              type="checkbox"
              checked={cfg.docreader_enabled}
              onChange={(e) => set({ docreader_enabled: e.target.checked })}
            />
            <span>随服务自动启停 DocReader 解析服务(启用 PDF 等复杂格式解析)</span>
          </label>
          <label className="field">
            DocReader 目录(留空自动使用部署目录内的 docreader\)
            <input
              className="mono"
              value={cfg.docreader_dir ?? ""}
              onChange={(e) => set({ docreader_dir: e.target.value || null })}
              placeholder="自动(部署目录\docreader)"
            />
            <span className="field-hint">
              便携包自带运行时,无需安装;开发目录需先执行 uv sync(含 main.py 与
              .venv)。监听 127.0.0.1:50051。
            </span>
          </label>
          <div className="btn-row">
            <button className="btn ghost" onClick={() => api.revealDataDir().catch(() => {})}>
              打开数据目录
            </button>
          </div>
        </section>

        <section className="card">
          <div className="card-title">部署诊断</div>
          {deploy && deploy.deploy_dir ? (
            <div className="kv">
              <div className="kv-row">
                <span className="kv-k">Lite 可执行</span>
                <Check ok={deploy.lite_found} />
              </div>
              <div className="kv-row">
                <span className="kv-k">Web 界面(web/)</span>
                <Check ok={deploy.web_ready} />
              </div>
              <div className="kv-row">
                <span className="kv-k">配置(config/)</span>
                <Check ok={deploy.config_ready} />
              </div>
              <div className="kv-row">
                <span className="kv-k">数据库迁移(migrations/)</span>
                <Check ok={deploy.migrations_ready} />
              </div>
              <div className="kv-row">
                <span className="kv-k">解析引擎(docreader/,PDF 等)</span>
                <Check ok={deploy.docreader_ready} />
              </div>
              <div className="kv-row">
                <span className="kv-k">部署目录</span>
                <span className="kv-v mono small">{deploy.deploy_dir}</span>
              </div>
            </div>
          ) : (
            <p className="muted">
              未找到部署目录。将后端程序(linkmind-lite.exe)、web/、config/、migrations/
              放入应用 runtime 目录,或在上方指定路径。
            </p>
          )}
        </section>
      </div>

        <section className="card">
          <div className="card-title">软件更新</div>
          <p className="muted" style={{ marginTop: 0 }}>
            当前版本 {appVersion || "…"}
            {update && !update.available && !update.portable && " — 已是最新"}
          </p>
          {update?.portable ? (
            <p className="muted">
              当前为 U 盘便携版,不支持在线更新。请获取新版便携包后,替换
              LinkMindDesktop.exe 与 runtime\ 下的程序文件(runtime\data\
              里的知识库数据不受影响)。
            </p>
          ) : (
            <>
              <div className="btn-row">
                <button className="btn ghost" disabled={checking || installing} onClick={checkUpdate}>
                  {checking ? "检查中…" : "检查更新"}
                </button>
                {update?.available && !installing && (
                  <button className="btn primary" onClick={doInstall}>
                    下载并安装 v{update.version}
                  </button>
                )}
              </div>
              {updateError && <p className="form-error">{updateError}</p>}
              {installing && progress && (
                <div>
                  <p className="muted" style={{ margin: "8px 0 0" }}>
                    {progress.stage === "downloading" && progress.total
                      ? `下载中 ${Math.round((progress.downloaded / progress.total) * 100)}%(${(progress.downloaded / 1048576).toFixed(1)} / ${(progress.total / 1048576).toFixed(1)} MB),完成后应用会自动重启`
                      : progress.stage === "downloading"
                        ? `下载中 ${(progress.downloaded / 1048576).toFixed(1)} MB…`
                        : progress.stage === "installing"
                          ? "下载完成,正在安装(应用将自动重启)…"
                          : "更新完成,即将重启应用…"}
                  </p>
                  {progress.stage === "downloading" && progress.total && (
                    <progress
                      className="update-progress"
                      value={progress.downloaded}
                      max={progress.total}
                    />
                  )}
                </div>
              )}
              {!installing && (
                <p className="field-hint">
                  在线更新适用于安装版;下载约 200MB,期间请勿关闭应用。
                </p>
              )}
            </>
          )}
        </section>

        <div className="btn-row save-bar">
          <button className="btn primary" disabled={saving} onClick={save}>
            {saving ? "保存中…" : "保存设置"}
          </button>
          {msg && (
            <span className={msg.ok ? "save-ok" : "form-error"}>{msg.text}</span>
          )}
        </div>
    </div>
  );
}

function Check({ ok }: { ok: boolean }) {
  return (
    <span className={"check " + (ok ? "ok" : "err")}>
      {ok ? "✓ 就绪" : "✗ 缺失"}
    </span>
  );
}
