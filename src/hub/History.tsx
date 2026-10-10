import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import {
  AlertCircle,
  Copy,
  Check,
  Download,
  HardDrive,
  History as HistoryIcon,
  Lock,
  MoreHorizontal,
  RotateCcw,
  Trash2,
} from "lucide-react";
import { api, isTauri } from "../lib/ipc";
import { EVENTS, type HistoryItem, type Mode } from "../lib/types";
import { useBackendEvent } from "./hooks";
import { useApp } from "./store";
import { useFeedback } from "./feedback";
import { DropdownMenu, PageTitle, Segmented, Select, Spinner, Tooltip } from "./ui";
import { RETENTION_OPTIONS, formatTime, groupLabel, localDateKey, retentionLabel, retentionRank } from "./format";
import type { HistoryRetention } from "../lib/types";

const PAGE = 50;

type Tab = "all" | Mode;
const TABS: { value: Tab; label: string }[] = [
  { value: "all", label: "全部" },
  { value: "dictation", label: "听写" },
  { value: "translation", label: "翻译" },
  { value: "ask", label: "问任何问题" },
];

const MODE_CHIP: Record<Mode, string | null> = { dictation: null, translation: "翻译", ask: "问任何问题" };

export function HistoryPage() {
  const { settings, update } = useApp();
  const { toast, confirm } = useFeedback();
  const [tab, setTab] = useState<Tab>("all");
  const [items, setItems] = useState<HistoryItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [hasMore, setHasMore] = useState(false);
  const [busy, setBusy] = useState<Set<string>>(new Set());
  const listRef = useRef<HTMLDivElement>(null);
  const sentinelRef = useRef<HTMLDivElement>(null);
  const token = useRef(0);
  const itemsLen = useRef(0);
  itemsLen.current = items.length;
  const mode: Mode | null = tab === "all" ? null : tab;

  /** Reload everything currently shown (at least one page). */
  const reload = useCallback(async () => {
    const mine = ++token.current;
    try {
      const limit = Math.max(PAGE, itemsLen.current);
      const rows = await api.listHistory({ mode, limit, offset: 0 });
      if (mine !== token.current) return;
      setItems(rows);
      setHasMore(rows.length >= limit);
    } finally {
      if (mine === token.current) setLoading(false);
    }
  }, [mode]);

  useEffect(() => {
    setItems([]);
    itemsLen.current = 0;
    setLoading(true);
    setHasMore(false);
    reload();
  }, [mode, reload]);
  useBackendEvent(EVENTS.historyUpdated, reload);

  const loadMore = useCallback(async () => {
    if (loading || !hasMore) return;
    const mine = token.current;
    setLoading(true);
    try {
      const rows = await api.listHistory({ mode, limit: PAGE, offset: itemsLen.current });
      if (mine !== token.current) return;
      setItems((cur) => {
        const seen = new Set(cur.map((i) => i.id));
        return [...cur, ...rows.filter((r) => !seen.has(r.id))];
      });
      setHasMore(rows.length >= PAGE);
    } finally {
      setLoading(false);
    }
  }, [loading, hasMore, mode]);

  useEffect(() => {
    const el = sentinelRef.current;
    const root = listRef.current;
    if (!el || !root) return;
    const io = new IntersectionObserver(
      (e) => {
        if (e[0].isIntersecting) loadMore();
      },
      { root, rootMargin: "200px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [loadMore, items.length]);

  const groups = useMemo(() => {
    const out: { key: string; label: string; rows: HistoryItem[] }[] = [];
    for (const it of items) {
      const key = localDateKey(new Date(it.createdAt));
      const last = out[out.length - 1];
      if (last && last.key === key) last.rows.push(it);
      else out.push({ key, label: groupLabel(key), rows: [it] });
    }
    return out;
  }, [items]);

  const setBusyId = (id: string, on: boolean) =>
    setBusy((b) => {
      const n = new Set(b);
      if (on) n.add(id);
      else n.delete(id);
      return n;
    });

  const retry = async (it: HistoryItem) => {
    setBusyId(it.id, true);
    try {
      const updated = await api.retryHistory(it.id);
      if (updated) setItems((cur) => cur.map((x) => (x.id === it.id ? updated : x)));
    } catch (e) {
      toast(`重试失败：${String(e)}`, "error");
    } finally {
      setBusyId(it.id, false);
    }
  };

  const remove = async (it: HistoryItem) => {
    const ok = await confirm({
      title: "删除此转录？",
      description: "此转录将被永久删除，无法恢复。",
      confirmLabel: "删除",
      danger: true,
    });
    if (!ok) return;
    try {
      await api.deleteHistory(it.id);
      setItems((cur) => cur.filter((x) => x.id !== it.id));
      toast("转录已删除");
    } catch {
      toast("删除转录失败。", "error");
    }
  };

  const download = async (it: HistoryItem) => {
    if (!isTauri) {
      toast("下载音频仅在桌面应用中可用", "info");
      return;
    }
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const d = new Date(it.createdAt);
      const stamp = `${localDateKey(d)}-${String(d.getHours()).padStart(2, "0")}${String(d.getMinutes()).padStart(2, "0")}`;
      const path = await save({ defaultPath: `hoshiro-${stamp}.wav`, filters: [{ name: "WAV 音频", extensions: ["wav"] }] });
      if (!path) return;
      await api.exportHistoryAudio(it.id, path);
      toast("音频已保存");
    } catch (e) {
      toast(`保存音频失败：${String(e)}`, "error");
    }
  };

  const deleteAll = async () => {
    const ok = await confirm({
      title: "删除所有历史记录？",
      description: "您所有的本地历史记录将被永久删除，无法恢复。",
      confirmLabel: "删除",
      danger: true,
    });
    if (!ok) return;
    try {
      await api.deleteAllHistory();
      setItems([]);
      setHasMore(false);
      toast("所有记录已删除");
    } catch {
      toast("删除所有历史记录失败", "error");
    }
  };

  const changeRetention = async (next: HistoryRetention) => {
    const cur = settings.historyRetention;
    if (next === cur) return;
    if (retentionRank(next) < retentionRank(cur)) {
      const never = next === "never";
      const ok = await confirm({
        title: never ? "从不保存历史？" : "删除旧历史记录？",
        description: never
          ? "您所有的本地历史记录将被永久删除，无法恢复。"
          : `您所有的本地历史记录早于 ${retentionLabel(next)} 将被永久删除，无法恢复。`,
        confirmLabel: "删除",
        danger: true,
      });
      if (!ok) return;
    }
    update((s) => void (s.historyRetention = next));
  };

  const never = settings.historyRetention === "never";

  return (
    <div className="history">
      <div className="history-head">
        <PageTitle
          right={
            <DropdownMenu
              items={[{ label: "删除所有", icon: <Trash2 size={15} />, danger: true, onClick: deleteAll }]}
              trigger={({ toggle, ref, open }) => (
                <button ref={ref} className={`icon-btn ${open ? "active" : ""}`} onClick={toggle} aria-label="更多">
                  <MoreHorizontal size={20} />
                </button>
              )}
            />
          }
        >
          历史记录
        </PageTitle>

        <div className="card settings-card">
          <div className="sc-row">
            <div className="sc-text">
              <div className="sc-title">
                <HardDrive size={15} /> 保存历史
              </div>
              <div className="sc-desc">选择在此设备上保留历史记录的时间。</div>
            </div>
            <Select
              value={settings.historyRetention}
              options={RETENTION_OPTIONS}
              onChange={changeRetention}
              width={140}
              align="end"
            />
          </div>
          <div className="sc-divider" />
          <div className="sc-row">
            <div className="sc-text">
              <div className="sc-title">
                <Lock size={15} /> 您的数据保持私密
              </div>
              <div className="sc-desc">录音和转录仅存储在此设备上，音频只会发送给您配置的语音识别与改写服务商。</div>
            </div>
          </div>
        </div>

        <div className="tabs-row">
          <Segmented id="hist-tabs" options={TABS} value={tab} onChange={setTab} />
        </div>
      </div>

      <div className="history-list" ref={listRef}>
        {never && items.length === 0 ? (
          <EmptyState title="从不保存历史" desc="您可以在顶部更改持续时间以开始保存口述历史。" />
        ) : !loading && items.length === 0 ? (
          <EmptyState title="尚无口述" desc="一旦您在不同应用中进行口述，您的所有口述历史将出现在这里。" />
        ) : (
          groups.map((g) => (
            <section key={g.key} className="hist-group">
              <div className="hist-group-label">{g.label}</div>
              <div className="hist-rows">
                {g.rows.map((it) => (
                  <HistoryRow
                    key={it.id}
                    item={it}
                    busy={busy.has(it.id)}
                    onRetry={() => retry(it)}
                    onDelete={() => remove(it)}
                    onDownload={() => download(it)}
                  />
                ))}
              </div>
            </section>
          ))
        )}
        <div ref={sentinelRef} className="hist-sentinel">
          {loading && items.length > 0 && <Spinner size={16} />}
        </div>
      </div>
    </div>
  );
}

function EmptyState({ title, desc }: { title: string; desc: string }) {
  return (
    <motion.div className="empty-state" initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }}>
      <div className="empty-icon">
        <HistoryIcon size={26} strokeWidth={1.6} />
      </div>
      <div className="empty-title">{title}</div>
      <div className="empty-desc">{desc}</div>
    </motion.div>
  );
}

function HistoryRow({
  item,
  busy,
  onRetry,
  onDelete,
  onDownload,
}: {
  item: HistoryItem;
  busy: boolean;
  onRetry: () => void;
  onDelete: () => void;
  onDownload: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [answer, setAnswer] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const stop = (e: React.MouseEvent) => e.stopPropagation();

  const finalText = item.refinedText ?? item.rawText ?? "";
  const copyText = async () => {
    if (!finalText) return;
    await api.copyText(finalText);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1400);
  };

  const processing = busy || item.status === "processing";
  const isAsk = item.mode === "ask";
  const chip = MODE_CHIP[item.mode];

  let body: React.ReactNode;
  if (processing) {
    body = (
      <span className="hist-status">
        <Spinner size={13} /> 正在处理…
      </span>
    );
  } else if (item.status === "dismissed") {
    body = (
      <span className="hist-status">
        转录已被取消。
        {item.hasAudio && (
          <button className="link-btn" onClick={(e) => (stop(e), onRetry())}>
            重试
          </button>
        )}
      </span>
    );
  } else if (item.status === "error") {
    body = (
      <span className="hist-status error">
        <AlertCircle size={14} />
        {item.errorMessage || "您的转录被中断。但 Hoshiro 仍然可以为您重试。"}
        {item.hasAudio && (
          <button className="link-btn" onClick={(e) => (stop(e), onRetry())}>
            重试
          </button>
        )}
      </span>
    );
  } else if (item.status === "empty") {
    body = (
      <Tooltip label="Hoshiro 没有检测到口述中的任何单词，因此此转录为空。">
        <span className="hist-status">音频无声。</span>
      </Tooltip>
    );
  } else if (isAsk) {
    body = (
      <div className="hist-ask">
        {item.selectedText && <blockquote className="hist-quote">{item.selectedText}</blockquote>}
        <div className="hist-text">{item.rawText || finalText}</div>
        {item.refinedText && (
          <>
            <button className="link-btn answer-toggle" onClick={(e) => (stop(e), setAnswer((a) => !a))}>
              {answer ? "收起答案" : "查看答案"}
            </button>
            <AnimatePresence initial={false}>
              {answer && (
                <motion.div
                  className="hist-answer"
                  initial={{ opacity: 0, height: 0 }}
                  animate={{ opacity: 1, height: "auto" }}
                  exit={{ opacity: 0, height: 0 }}
                  transition={{ duration: 0.18 }}
                  onClick={stop}
                >
                  <div className="hist-answer-inner">{item.refinedText}</div>
                </motion.div>
              )}
            </AnimatePresence>
          </>
        )}
      </div>
    );
  } else {
    body = <div className="hist-text">{finalText}</div>;
  }

  const menuItems = [
    ...(item.hasAudio ? [{ label: "重试", icon: <RotateCcw size={15} />, onClick: onRetry }] : []),
    ...(item.hasAudio ? [{ label: "下载音频", icon: <Download size={15} />, onClick: onDownload }] : []),
    { label: "删除转录", icon: <Trash2 size={15} />, danger: true, onClick: onDelete },
  ];

  return (
    <div className={`hist-row ${open ? "open" : ""} ${menuOpen ? "menu-open" : ""}`} onClick={() => setOpen((o) => !o)}>
      <div className="hist-time">
        <span>{formatTime(item.createdAt)}</span>
        {chip && <span className="hist-chip">{chip}</span>}
      </div>
      <div className="hist-main">
        {body}
        <AnimatePresence initial={false}>
          {open && (
            <motion.div
              className="hist-raw"
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: "auto" }}
              exit={{ opacity: 0, height: 0 }}
              transition={{ duration: 0.18 }}
              onClick={stop}
            >
              <div className="hist-raw-inner">
                <div className="hist-raw-label">原始转录</div>
                <div className="hist-raw-text">{item.rawText || "（无）"}</div>
                <div className="hist-meta">
                  {[
                    item.appName,
                    item.durationSecs > 0 ? `${item.durationSecs.toFixed(1)} 秒` : null,
                    item.source === "typeless" ? "从 Typeless 导入" : null,
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </div>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
      <div className="hist-actions" onClick={stop}>
        {finalText && item.status === "completed" && (
          <Tooltip label="已复制" forceOpen={copied} delay={99999}>
            <button className="icon-btn sm" onClick={copyText} aria-label="复制转录">
              {copied ? <Check size={15} /> : <Copy size={15} />}
            </button>
          </Tooltip>
        )}
        <DropdownMenu
          items={menuItems}
          onOpenChange={setMenuOpen}
          trigger={({ toggle, ref }) => (
            <button ref={ref} className="icon-btn sm" onClick={toggle} aria-label="更多">
              <MoreHorizontal size={16} />
            </button>
          )}
        />
      </div>
    </div>
  );
}
