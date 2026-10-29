import { useEffect, useRef, useState } from "react";
import { fmtTime, type LogLine } from "../api";

export default function LogsPanel({
  logs,
  onClear,
}: {
  logs: LogLine[];
  onClear: () => void;
}) {
  const [follow, setFollow] = useState(true);
  const boxRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (follow && boxRef.current) {
      boxRef.current.scrollTop = boxRef.current.scrollHeight;
    }
  }, [logs, follow]);

  return (
    <section className="card logs-card">
      <div className="card-title-row">
        <div className="card-title">服务日志</div>
        <div className="logs-tools">
          <label className="switch">
            <input
              type="checkbox"
              checked={follow}
              onChange={(e) => setFollow(e.target.checked)}
            />
            <span>自动滚动</span>
          </label>
          <button className="btn ghost small" onClick={onClear}>
            清空
          </button>
        </div>
      </div>
      <div className={"log-box" + (follow ? " follow" : "")} ref={boxRef}>
        {logs.length === 0 && <div className="log-empty">暂无日志</div>}
        {logs.map((l, i) => (
          <div key={i} className={"log-line " + l.stream}>
            <span className="log-ts">{fmtTime(l.ts)}</span>
            <span className="log-stream">[{l.stream}]</span>
            <span className="log-text">{l.line}</span>
          </div>
        ))}
      </div>
    </section>
  );
}
