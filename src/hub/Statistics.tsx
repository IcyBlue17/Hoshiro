import { useEffect, useRef, useState } from "react";
import { motion } from "motion/react";
import { api } from "../lib/ipc";
import { EVENTS, type UsageStats } from "../lib/types";
import { useBackendEvent } from "./hooks";
import { BigNumber } from "./Home";
import { useApp } from "./store";
import { Segmented, Spinner } from "./ui";
import { formatDuration, formatWords, shortcutLabel } from "./format";
import { AppsCard, AsrTable, LlmTable, ModeCard, OutcomeCard, SpeedCard, TimeCard, TrendCard } from "./stats/cards";
import { num, tokenText } from "./stats/util";

const rise = (i: number) => ({
  initial: { opacity: 0, y: 10 },
  animate: { opacity: 1, y: 0 },
  transition: { duration: 0.28, delay: 0.04 * i, ease: "easeOut" as const },
});

export function StatisticsPage() {
  const { settings } = useApp();
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

  const sc = settings.shortcuts;
  const shortcuts = {
    dictation: sc.dictation[0] ? shortcutLabel(sc.dictation[0]) : "",
    translation: sc.translation[0] ? shortcutLabel(sc.translation[0]) : "",
    askAnything: sc.askAnything[0] ? shortcutLabel(sc.askAnything[0]) : "",
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
      {data && <Body data={data} range={Number(range)} shortcuts={shortcuts} />}
    </div>
  );
}

function Body({ data, range, shortcuts }: { data: UsageStats; range: number; shortcuts: Record<string, string> }) {
  const { total, llmTotal } = data;
  const llmTokens = llmTotal.inputTokens + llmTotal.outputTokens;
  const saved = Math.max(0, (total.words / 40) * 60 - total.audioSecs);
  return (
    <div className="us-stack">
      <div className="us-kpis">
        <motion.div className="card stat-card" {...rise(0)}>
          <BigNumber parts={[{ value: formatWords(total.words), unit: "字" }]} />
          <div className="stat-label">生成文字</div>
          <div className="usage-detail">{num(data.completedSessions)} 次已完成会话</div>
        </motion.div>
        <motion.div className="card stat-card" {...rise(1)}>
          <BigNumber parts={formatDuration(total.audioSecs)} />
          <div className="stat-label">已完成转录时长</div>
          <div className="usage-detail">语音识别 {num(data.asrTotal.requests)} 次请求</div>
        </motion.div>
        <motion.div className="card stat-card" {...rise(2)}>
          <BigNumber parts={formatDuration(saved)} />
          <div className="stat-label">预计节省打字时间</div>
          <div className="usage-detail">按每分钟 40 字估算</div>
        </motion.div>
        <motion.div className="card stat-card" {...rise(3)}>
          <BigNumber parts={[{ value: tokenText(llmTokens), unit: "tokens" }]} />
          <div className="stat-label">改写模型用量</div>
          <div className="usage-detail">
            输入 {tokenText(llmTotal.inputTokens)} · 输出 {tokenText(llmTotal.outputTokens)}
            {llmTotal.cachedTokens > 0 && <> · 缓存 {tokenText(llmTotal.cachedTokens)}</>}
          </div>
        </motion.div>
      </div>

      <motion.div {...rise(4)}>
        <TrendCard stats={data} range={range} />
      </motion.div>

      <motion.div className="us-grid" {...rise(5)}>
        <SpeedCard stats={data} />
        <OutcomeCard stats={data} />
      </motion.div>

      <motion.div className="us-grid" {...rise(6)}>
        <ModeCard stats={data} shortcuts={shortcuts} />
        <AppsCard stats={data} />
      </motion.div>

      <motion.div {...rise(7)}>
        <TimeCard stats={data} />
      </motion.div>

      <motion.div {...rise(8)}>
        <AsrTable rows={data.asrModels} />
      </motion.div>
      <motion.div {...rise(9)}>
        <LlmTable rows={data.llmModels} />
      </motion.div>

      <div className="us-foot">
        <span><b>{num(data.dictionaryWords)}</b> 个词典词条（自动学习 {num(data.autoWords)} 个）</span>
        <span>所选期间失败的会话 <b>{num(data.errorSessions)}</b></span>
      </div>
    </div>
  );
}
