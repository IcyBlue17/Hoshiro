import { useEffect, useRef, useState } from "react";
import { api } from "../lib/ipc";
import { EVENTS, type UsageBucket, type UsageStats } from "../lib/types";
import { useBackendEvent } from "./hooks";
import { BigNumber } from "./Home";
import { Segmented, Spinner } from "./ui";
import { formatDuration, formatWords } from "./format";

const number = (n: number) => n.toLocaleString("zh-CN");
const modes: Record<string, string> = {
  dictation: "口述",
  translation: "翻译",
  ask: "问任何问题",
};

export function StatisticsPage() {
  const [range, setRange] = useState("30");
  const [data, setData] = useState<UsageStats | null>(null);
  const [error, setError] = useState("");
  const sequence = useRef(0);
  const load = () => {
    const seq = ++sequence.current;
    api
      .getUsage(Number(range))
      .then((d) => {
        if (seq === sequence.current) {
          setData(d);
          setError("");
        }
      })
      .catch((e) => {
        if (seq === sequence.current) setError(String(e));
      });
  };
  useEffect(() => {
    setData(null);
    load();
    return () => {
      sequence.current++;
    };
  }, [range]);
  useBackendEvent(EVENTS.statsUpdated, load);
  const total = data?.total;
  const chartDays = data?.days.slice(-60) ?? [];
  const chartMax = Math.max(1, ...chartDays.map((d) => d.words));
  const bars = (items: UsageBucket[]) => {
    const max = Math.max(1, ...items.map((x) => x.words));
    return items.length ? (
      <div className="usage-breakdown">
        {items.map((x) => (
          <div key={x.label}>
            <div className="usage-row">
              <span>{modes[x.label] ?? x.label}</span>
              <span>{number(x.words)} 字</span>
            </div>
            <div className="usage-track">
              <div style={{ width: `${(100 * x.words) / max}%` }} />
            </div>
          </div>
        ))}
      </div>
    ) : (
      <p className="muted">还没有使用记录</p>
    );
  };
  return (
    <div className="page-scroll usage-page">
      <div className="usage-heading">
        <h1 className="page-title">使用统计</h1>
        <Segmented
          id="usage-range"
          value={range}
          onChange={setRange}
          options={[
            { value: "7", label: "近 7 天" },
            { value: "30", label: "近 30 天" },
            { value: "0", label: "全部" },
          ]}
        />
      </div>
      <p className="muted">看看您说了多少，写了多少，以及 AI 为您做了多少。</p>
      {error && (
        <p role="alert">
          加载失败：{error}{" "}
          <button className="link-btn" onClick={load}>
            重试
          </button>
        </p>
      )}
      {!data && !error && <Spinner />}
      {data && total && (
        <>
          <div className="stat-grid">
            <div className="card stat-card">
              <BigNumber
                parts={[
                  {
                    value: formatWords(total.inputTokens + total.outputTokens),
                    unit: "tokens",
                  },
                ]}
              />
              <div className="stat-label">服务商已报告的用量</div>
              <div className="usage-detail">
                输入 {number(total.inputTokens)} · 输出{" "}
                {number(total.outputTokens)}
              </div>
            </div>
            <div className="card stat-card">
              <BigNumber parts={formatDuration(total.audioSecs)} />
              <div className="stat-label">已完成转录时长</div>
              <div className="usage-detail">
                {number(data.completedSessions)} 次已完成会话
              </div>
            </div>
            <div className="card stat-card">
              <BigNumber
                parts={[{ value: formatWords(total.words), unit: "字" }]}
              />
              <div className="stat-label">生成文字</div>
              <div className="usage-detail">口述、翻译和问答</div>
            </div>
            <div className="card stat-card">
              <BigNumber
                parts={formatDuration(
                  Math.max(0, (total.words / 40) * 60 - total.audioSecs),
                )}
              />
              <div className="stat-label">预计节省打字时间</div>
              <div className="usage-detail">按每分钟 40 字估算</div>
            </div>
          </div>
          <section className="card usage-section">
            <h2 className="section-title">每天的表达</h2>
            {data.days.length ? (
              <div
                className="usage-chart"
                role="img"
                aria-label="每日生成字数，悬停查看详情"
              >
                {chartDays.map((d) => (
                  <div
                    className="usage-day"
                    key={d.label}
                    title={`${d.label}：${number(d.words)} 字 · ${d.sessions} 次 · ${number(d.inputTokens + d.outputTokens)} tokens`}
                  >
                    <div
                      className="usage-day-bar"
                      style={{
                        height: `${Math.max(2, (100 * d.words) / chartMax)}%`,
                      }}
                    />
                    <span>{d.label.slice(5)}</span>
                  </div>
                ))}
              </div>
            ) : (
              <p className="muted">
                开始一次口述，您的使用趋势就会出现在这里。
              </p>
            )}
            <p className="usage-detail">
              按本地日期显示，最多展示最近 60 个有记录的日期。
            </p>
          </section>
          <div className="usage-columns">
            <section className="card usage-section">
              <h2 className="section-title">使用方式</h2>
              {bars(data.modes)}
            </section>
            <section className="card usage-section">
              <h2 className="section-title">常用应用</h2>
              {bars(data.apps)}
            </section>
          </div>
          <section className="card usage-section">
            <h2 className="section-title">模型用量</h2>
            <div className="usage-table-wrap">
              <table className="usage-table">
                <thead>
                  <tr>
                    <th>服务 / 模型</th>
                    <th>请求</th>
                    <th>输入 token</th>
                    <th>输出 token</th>
                    <th>送识别时长</th>
                  </tr>
                </thead>
                <tbody>
                  {data.providers.map((p) => (
                    <tr key={p.label}>
                      <td>
                        {p.label}
                        {p.unreportedRequests > 0 && (
                          <div className="usage-detail">
                            {p.unreportedRequests} 次未完整报告 token
                          </div>
                        )}
                      </td>
                      <td>{number(p.requests)}</td>
                      <td>{number(p.inputTokens)}</td>
                      <td>{number(p.outputTokens)}</td>
                      <td>{Math.round(p.audioSecs)} 秒</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            {!data.providers.length && (
              <p className="muted">还没有记录到模型请求</p>
            )}
            <p className="usage-detail">
              Token
              仅累计服务商实际返回的用量，包含连接测试和个性化分析；不估算旧记录或未报告的部分。重试请求会单独计入。实际计费以服务商账单为准。
            </p>
          </section>
          <section className="card usage-section usage-footnotes">
            <div>
              <b>{number(data.dictionaryWords)}</b>
              <span>词典中的词 · 自动学习 {number(data.autoWords)} 个</span>
            </div>
            <div>
              <b>{number(data.errorSessions)}</b>
              <span>所选期间失败的会话</span>
            </div>
          </section>
          <p className="usage-detail">
            会话、文字、时长来自当前保留的历史（包含导入记录），删除历史会影响这些统计。模型用量独立保留，不包含文本内容；词典展示当前总量。
          </p>
        </>
      )}
    </div>
  );
}
