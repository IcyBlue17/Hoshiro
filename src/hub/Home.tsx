import { useCallback, useEffect, useState } from "react";
import { motion } from "motion/react";
import { AlertTriangle } from "lucide-react";
import { api } from "../lib/ipc";
import { EVENTS, type Stats, type TypelessDetection } from "../lib/types";
import { useBackendEvent, usePoll } from "./hooks";
import { formatDuration, formatWords } from "./format";
import { useApp } from "./store";
import { ShortcutKeys } from "./ui";
import { useFeedback } from "./feedback";
import { Heatmap } from "./Heatmap";

function BigNumber({ parts }: { parts: { value: string | number; unit: string }[] }) {
  return (
    <div className="stat-number">
      {parts.map((p, i) => (
        <span key={i} className="stat-part">
          <span className="stat-value">{p.value}</span>
          <span className="stat-unit">{p.unit}</span>
        </span>
      ))}
    </div>
  );
}

const EMPTY_STATS: Stats = {
  totalWords: 0,
  totalDictationSecs: 0,
  averageWpm: 0,
  timeSavedSecs: 0,
  activeDays: 0,
  currentStreak: 0,
  longestStreak: 0,
  heatmap: [],
};

export function Home() {
  const { settings, version } = useApp();
  const { toast } = useFeedback();
  const [stats, setStats] = useState<Stats>(EMPTY_STATS);
  const [typeless, setTypeless] = useState<TypelessDetection | null>(null);
  const [quitting, setQuitting] = useState(false);

  const load = useCallback(() => {
    api.getStats().then(setStats).catch(() => {});
  }, []);
  useEffect(load, [load]);
  useBackendEvent(EVENTS.statsUpdated, load);
  useBackendEvent(EVENTS.historyUpdated, load);

  const detect = useCallback(() => {
    api.detectTypeless().then(setTypeless).catch(() => {});
  }, []);
  usePoll(detect, 5000);

  const quit = async () => {
    setQuitting(true);
    try {
      await api.quitTypeless();
      await new Promise((r) => setTimeout(r, 600));
      detect();
    } finally {
      setQuitting(false);
    }
  };

  const sc = settings.shortcuts;
  const shortcutRows: { label: string; list: string[] }[] = [
    { label: "口述", list: sc.dictation },
    { label: "翻译", list: sc.translation },
    { label: "问任何问题", list: sc.askAnything },
  ];

  const stagger = (i: number) => ({
    initial: { opacity: 0, y: 10 },
    animate: { opacity: 1, y: 0 },
    transition: { duration: 0.28, delay: 0.04 * i, ease: "easeOut" as const },
  });

  return (
    <div className="page-scroll home">
      <div className="hero-glow" />
      <div className="home-layout">
        <div className="home-main">
          <h1 className="hero-title">说话，不要打字</h1>

          {typeless?.running && (
            <motion.div className="banner warn" {...stagger(0)}>
              <AlertTriangle size={18} />
              <span className="banner-text">Typeless 正在运行，会与 Hoshiro 抢占 Fn 键</span>
              <button className="btn btn-sm btn-secondary" onClick={quit} disabled={quitting}>
                {quitting ? "正在退出…" : "退出 Typeless"}
              </button>
            </motion.div>
          )}

          <section className="insights">
            <div className="insights-head">
              <h2 className="section-title">洞察</h2>
            </div>

            <div className="stat-grid">
              <motion.div className="card stat-card" {...stagger(1)}>
                <BigNumber parts={[{ value: formatWords(stats.totalWords), unit: "字" }]} />
                <div className="stat-label">口述字数</div>
              </motion.div>
              <motion.div className="card stat-card" {...stagger(2)}>
                <BigNumber parts={formatDuration(stats.timeSavedSecs)} />
                <div className="stat-label">节省时间</div>
              </motion.div>
              <motion.div className="card stat-card" {...stagger(3)}>
                <BigNumber parts={[{ value: Math.round(stats.averageWpm), unit: "每分钟字数" }]} />
                <div className="stat-label">平均口述速度</div>
              </motion.div>
              <motion.div className="card stat-card" {...stagger(4)}>
                <BigNumber parts={formatDuration(stats.totalDictationSecs)} />
                <div className="stat-label">总口述时间</div>
              </motion.div>
            </div>

            <motion.div className="card activity-card" {...stagger(5)}>
              <div className="activity-stats">
                <div>
                  <BigNumber parts={[{ value: stats.activeDays, unit: "天" }]} />
                  <div className="stat-label">活跃天数</div>
                </div>
                <div>
                  <BigNumber parts={[{ value: stats.currentStreak, unit: "天" }]} />
                  <div className="stat-label">当前连续天数</div>
                </div>
                <div>
                  <BigNumber parts={[{ value: stats.longestStreak, unit: "天" }]} />
                  <div className="stat-label">最长连续天数</div>
                </div>
              </div>
              <Heatmap days={stats.heatmap} />
            </motion.div>
          </section>
        </div>

        <aside className="home-right">
          <motion.div className="card shortcut-card" {...stagger(2)}>
            {shortcutRows.map((row) => (
              <div className="shortcut-row" key={row.label}>
                <div className="shortcut-label">{row.label}</div>
                <div className="shortcut-keys">
                  {row.list.length === 0 && <span className="muted">未设置</span>}
                  {row.list.map((s, i) => (
                    <span className="shortcut-alt" key={s}>
                      {i > 0 && <span className="shortcut-or">或</span>}
                      <ShortcutKeys shortcut={s} />
                    </span>
                  ))}
                </div>
              </div>
            ))}
          </motion.div>

          <div className="home-version">
            <span>版本 v{version || "0.1.0"}</span>
            <button className="link-btn" onClick={() => toast("暂不支持自动检查更新", "info")}>
              检查更新
            </button>
          </div>
        </aside>
      </div>
    </div>
  );
}
