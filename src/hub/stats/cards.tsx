import { useEffect, useMemo, useState, type ReactNode } from "react";
import type { UsageBucket, UsageStats } from "../../lib/types";
import { formatWords } from "../format";
import { fillDays, initial, minutesText, msParts, msText, nameHue, niceMax, num, splitLabel, tokenText } from "./util";

/** Flips to true one frame after mount so CSS transitions animate from the collapsed state. */
function useRise(dep?: unknown): boolean {
  const [up, setUp] = useState(false);
  useEffect(() => {
    setUp(false);
    const id = requestAnimationFrame(() => requestAnimationFrame(() => setUp(true)));
    return () => cancelAnimationFrame(id);
  }, [dep]);
  return up;
}

export function Card({ title, sub, children, className = "" }: { title: string; sub?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={`card us-card ${className}`}>
      <div className="us-head">
        <h2>{title}</h2>
        {sub && <span className="us-sub">{sub}</span>}
      </div>
      {children}
    </section>
  );
}

const Empty = ({ children }: { children: ReactNode }) => <div className="us-empty">{children}</div>;

// ───────────────────────── trend ─────────────────────────

export function TrendCard({ stats, range }: { stats: UsageStats; range: number }) {
  const days = useMemo(() => fillDays(stats.days, range), [stats.days, range]);
  const up = useRise(days.length);
  const [hover, setHover] = useState<number | null>(null);
  const hasData = stats.days.some((d) => d.words > 0);
  const max = niceMax(Math.max(0, ...days.map((d) => d.words)));
  const n = days.length;
  const ticks = Array.from(new Set(Array.from({ length: Math.min(6, n) }, (_, i) => Math.round((i * (n - 1)) / Math.max(1, Math.min(6, n) - 1)))));
  const h = hover !== null ? days[hover] : null;
  return (
    <Card title="每日趋势" sub={hasData ? `生成字数 · 共 ${n} 天` : undefined}>
      {!hasData ? (
        <Empty>开始一次口述，您每天的使用趋势就会出现在这里。</Empty>
      ) : (
        <div className="us-chart" onMouseLeave={() => setHover(null)}>
          <div className="us-yaxis">
            {[0, 0.5, 1].map((f) => (
              <span key={f} style={{ top: `${(1 - f) * 100}%` }}>{formatWords(max * f)}</span>
            ))}
          </div>
          <div className="us-plot">
            {[0.5, 1].map((f) => (
              <div key={f} className="us-grid-line" style={{ top: `${(1 - f) * 100}%` }} />
            ))}
            <div className="us-grid-line base" style={{ top: "100%" }} />
            {days.map((d, i) => (
              <div key={d.label} className={`us-col${hover === i ? " on" : ""}`} onMouseEnter={() => setHover(i)}>
                <div
                  className={`us-bar${up ? " up" : ""}${d.words === 0 ? " zero" : ""}`}
                  style={{ height: `${Math.max(0, (100 * d.words) / max)}%`, transitionDelay: `${Math.min(i * 12, 420)}ms` }}
                />
              </div>
            ))}
            {h && (
              <div className="us-tip" style={{ left: `clamp(76px, ${((hover! + 0.5) / n) * 100}%, calc(100% - 76px))` }}>
                <b>{h.label.slice(5).replace("-", " 月 ")} 日</b>
                <div><span>生成字数</span><span>{num(h.words)} 字</span></div>
                <div><span>会话</span><span>{h.sessions} 次</span></div>
                <div><span>音频</span><span>{minutesText(h.audioSecs)} 分钟</span></div>
              </div>
            )}
          </div>
          <div className="us-xaxis">
            {ticks.map((i, k) => (
              <span
                key={i}
                className={k === 0 ? "first" : k === ticks.length - 1 ? "last" : ""}
                style={{ left: `${((i + 0.5) / n) * 100}%`, ...(k === 0 ? { left: 0 } : k === ticks.length - 1 ? { left: "100%" } : {}) }}
              >
                {days[i].label.slice(5).replace("-", "/")}
              </span>
            ))}
          </div>
        </div>
      )}
    </Card>
  );
}

// ───────────────────────── speed ─────────────────────────

export function SpeedCard({ stats }: { stats: UsageStats }) {
  const { total, asr, llm } = stats.latency;
  if (total.samples === 0) {
    return (
      <Card title="速度">
        <Empty>从现在起，每次会话的耗时都会记录在这里。<br />之前的历史没有耗时数据。</Empty>
      </Card>
    );
  }
  const asrMs = asr.meanMs, llmMs = llm.meanMs;
  const other = Math.max(0, total.meanMs - asrMs - llmMs);
  const sum = Math.max(1, asrMs + llmMs + other);
  const parts = [
    { key: "asr", label: "语音识别", ms: asrMs, color: "var(--us-asr)" },
    { key: "llm", label: "改写", ms: llmMs, color: "var(--us-llm)" },
    { key: "other", label: "上屏等其他", ms: other, color: "var(--us-other)" },
  ].filter((p) => p.ms > 0 || p.key !== "other");
  let hint = "识别与改写耗时接近";
  if (asrMs + llmMs > 0) {
    if (asrMs / (asrMs + llmMs) >= 0.58) hint = "大部分时间花在语音识别上";
    else if (llmMs / (asrMs + llmMs) >= 0.58) hint = "大部分时间花在改写上";
  }
  if (llm.samples === 0) hint = "未启用改写模型，耗时几乎全部是语音识别";
  const big = msParts(total.meanMs);
  return (
    <Card title="速度" sub={`基于 ${num(total.samples)} 次会话`}>
      <div className="us-big"><b>{big.value}</b><span>{big.unit}</span></div>
      <div className="us-meta" style={{ marginBottom: 14 }}>
        平均松手到上屏耗时 · 中位 <b>{msText(total.medianMs)}</b> · P90 <b>{msText(total.p90Ms)}</b>
      </div>
      <div className="us-split" role="img" aria-label="识别与改写耗时占比">
        {parts.map((p) => (
          <i key={p.key} style={{ flexGrow: Math.max(p.ms, sum * 0.01), flexBasis: 0, background: p.color }} />
        ))}
      </div>
      <div className="us-legend">
        {parts.map((p) => (
          <div className="us-leg" key={p.key}>
            <span className="dot" style={{ background: p.color }} />
            <span>{p.label}</span>
            <span className="val"><b>{msText(p.ms)}</b> · {Math.round((100 * p.ms) / sum)}%</span>
          </div>
        ))}
      </div>
      <div className="us-hint">{hint}</div>
    </Card>
  );
}

// ───────────────────────── outcomes ─────────────────────────

export function OutcomeCard({ stats }: { stats: UsageStats }) {
  const o = stats.outcomes;
  const all = o.completed + o.dismissed + o.error + o.empty;
  if (all === 0) {
    return (
      <Card title="结果">
        <Empty>还没有会话记录。</Empty>
      </Card>
    );
  }
  const rate = Math.round((100 * o.completed) / all);
  const items = [
    { label: "完成", n: o.completed, color: "var(--success)" },
    { label: "取消", n: o.dismissed, color: "var(--text-3)" },
    { label: "失败", n: o.error, color: "var(--danger)" },
    { label: "无声", n: o.empty, color: "var(--warning)" },
  ];
  return (
    <Card title="结果" sub={`共 ${num(all)} 次会话`}>
      <div className="us-rate">
        <div className="us-big"><b>{rate}</b><span>%</span><span className="us-meta" style={{ marginLeft: 6, fontWeight: 400 }}>成功率（完成 / 全部会话）</span></div>
      </div>
      <div className="us-split" role="img" aria-label="会话结果占比">
        {items.filter((i) => i.n > 0).map((i) => (
          <i key={i.label} style={{ flexGrow: i.n, flexBasis: 0, background: i.color }} />
        ))}
      </div>
      <div className="us-legend">
        {items.map((i) => (
          <div className={`us-leg${i.n === 0 ? " dim" : ""}`} key={i.label}>
            <span className="dot" style={{ background: i.color }} />
            <span>{i.label}</span>
            <span className="val"><b>{num(i.n)}</b> 次</span>
          </div>
        ))}
      </div>
    </Card>
  );
}

// ───────────────────────── modes ─────────────────────────

const MODES = [
  { key: "dictation", label: "口述", color: "var(--us-asr)", shortcut: "dictation" },
  { key: "translation", label: "翻译", color: "var(--us-llm)", shortcut: "translation" },
  { key: "ask", label: "问任何问题", color: "var(--warning)", shortcut: "askAnything" },
] as const;

export function ModeCard({ stats, shortcuts }: { stats: UsageStats; shortcuts: Record<string, string> }) {
  const rows = MODES.map((m) => ({ ...m, b: stats.modes.find((x) => x.label === m.key) }));
  const used = rows.filter((r) => (r.b?.sessions ?? 0) > 0);
  const total = used.reduce((s, r) => s + (r.b?.sessions ?? 0), 0);
  const unused = rows.filter((r) => !(r.b?.sessions ?? 0));
  return (
    <Card title="使用方式" sub={total ? `${num(total)} 次已完成` : undefined}>
      {total === 0 ? (
        <Empty>完成一次口述、翻译或提问后，这里会显示您的使用习惯。</Empty>
      ) : (
        <>
          <div className="us-split" role="img" aria-label="使用方式占比">
            {used.map((r) => (
              <i key={r.key} style={{ flexGrow: r.b!.sessions, flexBasis: 0, background: r.color }} />
            ))}
          </div>
          <div className="us-legend">
            {rows.map((r) => (
              <div className={`us-leg${r.b?.sessions ? "" : " dim"}`} key={r.key}>
                <span className="dot" style={{ background: r.color }} />
                <span>{r.label}</span>
                <span className="val">
                  {r.b?.sessions ? (<><b>{num(r.b.sessions)}</b> 次 · {num(r.b.words)} 字</>) : "尚未使用"}
                </span>
              </div>
            ))}
          </div>
          <div className="us-meta" style={{ marginTop: 14 }}>
            平均每次 <b>{Math.round(stats.averageWordsPerSession)}</b> 字 · 平均 <b>{Math.round(stats.averageSessionSecs)}</b> 秒 · 最长 <b>{Math.round(stats.longestSessionSecs)}</b> 秒
          </div>
          {unused.length > 0 && used.length === 1 && (
            <div className="us-hint">
              还可以试试{unused.map((u) => `${u.label}（${shortcuts[u.shortcut] || "未设置快捷键"}）`).join("、")}。
            </div>
          )}
        </>
      )}
    </Card>
  );
}

// ───────────────────────── apps ─────────────────────────

export function AppsCard({ stats }: { stats: UsageStats }) {
  const apps = stats.apps.slice(0, 6);
  const up = useRise(stats.total.words);
  const all = Math.max(1, stats.total.words);
  const top = Math.max(1, ...apps.map((a) => a.words));
  return (
    <Card title="常用应用" sub={apps.length ? "按生成字数" : undefined}>
      {apps.length === 0 ? (
        <Empty>还没有应用使用记录。</Empty>
      ) : (
        <div className="us-apps">
          {apps.map((a) => {
            const unknown = a.label === "未知应用";
            const color = unknown ? "var(--text-3)" : `hsl(${nameHue(a.label)} 58% 46%)`;
            return (
              <div className="us-app" key={a.label}>
                <div className="us-avatar" style={{ background: color }} aria-hidden>
                  {unknown ? "?" : initial(a.label)}
                </div>
                <div style={{ minWidth: 0 }}>
                  <div className="us-app-top">
                    <span>{a.label}</span>
                    <span>{num(a.words)} 字 · {a.sessions} 次</span>
                  </div>
                  <div className="us-track">
                    <i style={{ width: up ? `${(100 * a.words) / top}%` : 0, background: color }} />
                  </div>
                </div>
                <div className="us-pct">{Math.round((100 * a.words) / all)}%</div>
              </div>
            );
          })}
        </div>
      )}
    </Card>
  );
}

// ───────────────────────── time of day ─────────────────────────

const WEEK = ["一", "二", "三", "四", "五", "六", "日"];

export function TimeCard({ stats }: { stats: UsageStats }) {
  const up = useRise(stats.completedSessions);
  const hours = stats.hours.length === 24 ? stats.hours : Array(24).fill(0);
  const week = stats.weekdays.length === 7 ? stats.weekdays : Array(7).fill(0);
  const hMax = Math.max(0, ...hours), wMax = Math.max(0, ...week);
  const peakH = hMax > 0 ? hours.indexOf(hMax) : -1;
  const peakW = wMax > 0 ? week.indexOf(wMax) : -1;
  const pad = (h: number) => String(h).padStart(2, "0");
  return (
    <Card
      title="使用时段"
      sub={peakH >= 0 ? <>高峰 {pad(peakH)}:00–{pad((peakH + 1) % 24)}:00 · 周{WEEK[peakW]}最常用</> : undefined}
    >
      {hMax === 0 ? (
        <Empty>有了使用记录后，这里会显示您常在几点、周几使用。</Empty>
      ) : (
        <div className="us-time">
          <div>
            <div className="us-mini" role="img" aria-label="24 小时使用分布">
              {hours.map((v, h) => (
                <div key={h} title={`${pad(h)}:00　${v} 次`}>
                  <i
                    className={`${up ? "up" : ""}${h === peakH ? " peak" : ""}${v === 0 ? " zero" : ""}`}
                    style={{ height: `${v === 0 ? 4 : Math.max(8, (100 * v) / hMax)}%`, transitionDelay: `${h * 14}ms` }}
                  />
                </div>
              ))}
            </div>
            <div className="us-mini-axis">
              {[0, 6, 12, 18].map((h) => (
                <span key={h} className={h === 0 ? "first" : ""} style={{ left: h === 0 ? 0 : `${((h + 0.5) / 24) * 100}%` }}>{pad(h)}:00</span>
              ))}
              <span className="last" style={{ left: "100%" }}>24:00</span>
            </div>
          </div>
          <div>
            <div className="us-wk" role="img" aria-label="一周使用分布">
              {week.map((v, d) => (
                <div key={d} title={`周${WEEK[d]}　${v} 次`}>
                  <div className="col">
                    <i
                      className={`${up ? "up" : ""}${d === peakW ? " peak" : ""}${v === 0 ? " zero" : ""}`}
                      style={{ height: `${v === 0 ? 4 : Math.max(8, (100 * v) / wMax)}%`, transitionDelay: `${d * 40}ms` }}
                    />
                  </div>
                  <span>{WEEK[d]}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </Card>
  );
}

// ───────────────────────── model tables ─────────────────────────

function Svc({ label, kind }: { label: string; kind: "asr" | "llm" }) {
  const [name, model] = splitLabel(label);
  return (
    <div className="us-svc">
      <b>{name}</b>
      {model && <span>{model}</span>}
      <span hidden>{kind}</span>
    </div>
  );
}

export function AsrTable({ rows }: { rows: UsageBucket[] }) {
  return (
    <Card title="语音识别" sub="按音频时长计费">
      {rows.length === 0 ? (
        <Empty>还没有语音识别请求。</Empty>
      ) : (
        <div className="us-table-wrap">
          <table className="us-table">
            <thead>
              <tr><th>服务 · 模型</th><th>请求次数</th><th>音频时长（分钟）</th><th>中位识别耗时</th></tr>
            </thead>
            <tbody>
              {rows.map((p) => (
                <tr key={p.label}>
                  <td><Svc label={p.label} kind="asr" /></td>
                  <td>{num(p.requests)}</td>
                  <td>{minutesText(p.audioSecs)}</td>
                  <td>{p.medianMs != null ? msText(p.medianMs) : "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Card>
  );
}

export function LlmTable({ rows }: { rows: UsageBucket[] }) {
  const hasCache = rows.some((r) => r.cachedTokens > 0);
  const unreported = rows.reduce((s, r) => s + r.unreportedRequests, 0);
  return (
    <Card title="改写模型" sub="按 token 计费">
      {rows.length === 0 ? (
        <Empty>还没有改写请求。启用改写模型后，用量会显示在这里。</Empty>
      ) : (
        <>
          <div className="us-table-wrap">
            <table className="us-table">
              <thead>
                <tr>
                  <th>服务 · 模型</th><th>请求</th><th>输入 token</th><th>输出 token</th>
                  {hasCache && <th>缓存命中</th>}
                  <th>中位耗时</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((p) => (
                  <tr key={p.label}>
                    <td>
                      <Svc label={p.label} kind="llm" />
                      {p.unreportedRequests > 0 && <span className="usage-detail">{p.unreportedRequests} 次未返回 token 数</span>}
                    </td>
                    <td>{num(p.requests)}</td>
                    <td>{tokenText(p.inputTokens)}</td>
                    <td>{tokenText(p.outputTokens)}</td>
                    {hasCache && <td>{p.cachedTokens > 0 ? tokenText(p.cachedTokens) : "—"}</td>}
                    <td>{p.medianMs != null ? msText(p.medianMs) : "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {unreported > 0 && <p className="usage-detail">有 {unreported} 次改写请求，服务商没有返回 token 用量，未计入上方数字。</p>}
        </>
      )}
    </Card>
  );
}
