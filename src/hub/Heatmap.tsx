import { useMemo, useRef, useState } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";
import type { HeatmapDay } from "../lib/types";
import { localDateKey } from "./format";

const COLS = 24;
const PAGE_WEEKS = 4;
const DAY_LABELS = ["S", "M", "T", "W", "T", "F", "S"];
const MONTHS = ["一月", "二月", "三月", "四月", "五月", "六月", "七月", "八月", "九月", "十月", "十一月", "十二月"];

interface Cell {
  key: string;
  date: Date;
  words: number;
  sessions: number;
  kind: "future" | "nodata" | "none" | "level";
  level: number;
}

const USAGE = ["", "轻度使用", "中度使用", "高使用频率", "强力使用"];

export function Heatmap({ days }: { days: HeatmapDay[] }) {
  const [offset, setOffset] = useState(0); // weeks back from the current week
  const [hover, setHover] = useState<{ cell: Cell; x: number; y: number } | null>(null);
  const wrapRef = useRef<HTMLDivElement>(null);

  const { columns, firstKey } = useMemo(() => {
    const byDate = new Map(days.map((d) => [d.date, d]));
    let max = 0;
    let first: string | null = null;
    for (const d of days) {
      if (d.words > 0 || d.sessions > 0) {
        if (d.words > max) max = d.words;
        if (first === null || d.date < first) first = d.date;
      }
    }
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const todayKey = localDateKey(today);
    const weekStart = new Date(today);
    weekStart.setDate(today.getDate() - today.getDay() - offset * 7);
    const cols: Cell[][] = [];
    for (let c = 0; c < COLS; c++) {
      const start = new Date(weekStart);
      start.setDate(weekStart.getDate() - (COLS - 1 - c) * 7);
      const col: Cell[] = [];
      for (let r = 0; r < 7; r++) {
        const date = new Date(start);
        date.setDate(start.getDate() + r);
        const key = localDateKey(date);
        const rec = byDate.get(key);
        const words = rec?.words ?? 0;
        const sessions = rec?.sessions ?? 0;
        let kind: Cell["kind"] = "none";
        let level = 0;
        if (key > todayKey) kind = "future";
        else if (first === null || key < first) kind = "nodata";
        else if (words > 0 || sessions > 0) {
          kind = "level";
          level = words > 0 ? Math.min(4, Math.max(1, Math.ceil((words / (max || 1)) * 4))) : 1;
        }
        col.push({ key, date, words, sessions, kind, level });
      }
      cols.push(col);
    }
    return { columns: cols, firstKey: first };
  }, [days, offset]);

  const labels: { col: number; text: string }[] = [];
  columns.forEach((col, i) => {
    if (i === 0) return;
    const hit = col.find((c) => c.date.getDate() === 1);
    if (hit) labels.push({ col: i, text: MONTHS[hit.date.getMonth()] });
  });

  const canPrev = firstKey !== null && columns[0][0].key > firstKey;
  const canNext = offset > 0;

  const show = (cell: Cell, el: HTMLElement) => {
    const w = wrapRef.current;
    if (!w) return;
    const wr = w.getBoundingClientRect();
    const er = el.getBoundingClientRect();
    setHover({ cell, x: er.left - wr.left + er.width / 2, y: er.top - wr.top });
  };

  const tip = hover?.cell;
  const tipText = !tip
    ? ""
    : tip.kind === "nodata" || tip.kind === "future"
      ? "暂无数据"
      : tip.kind === "none"
        ? "没有活动"
        : USAGE[tip.level];

  return (
    <div className="heat" ref={wrapRef} onMouseLeave={() => setHover(null)}>
      <div className="heat-grid" style={{ gridTemplateColumns: `14px repeat(${COLS}, 1fr)` }}>
        {DAY_LABELS.map((lab, r) => (
          <FragmentRow key={r} label={lab} r={r} columns={columns} onHover={show} />
        ))}
      </div>
      <div className="heat-months">
        <button className="heat-arrow" disabled={!canPrev} onClick={() => setOffset((o) => o + PAGE_WEEKS)} aria-label="上一页">
          <ChevronLeft size={15} />
        </button>
        <div className="heat-month-labels">
          {labels.map((l) => (
            <span key={l.col} style={{ left: `${(l.col / COLS) * 100}%` }}>
              {l.text}
            </span>
          ))}
        </div>
        <button
          className="heat-arrow"
          disabled={!canNext}
          onClick={() => setOffset((o) => Math.max(0, o - PAGE_WEEKS))}
          aria-label="下一页"
        >
          <ChevronRight size={15} />
        </button>
      </div>
      <div className="heat-legend">
        <span>更少</span>
        {[1, 2, 3, 4].map((l) => (
          <i key={l} className={`heat-cell lv${l}`} />
        ))}
        <span>更多</span>
      </div>
      {hover && tip && (
        <div className="heat-tip" style={{ left: hover.x, top: hover.y }}>
          <b>
            {tip.date.getFullYear()}年{tip.date.getMonth() + 1}月{tip.date.getDate()}日
          </b>
          <span>
            {tip.kind === "level"
              ? tip.words > 0
                ? `${tip.words.toLocaleString()} 字 · ${tipText}`
                : `${tip.sessions} 次会话`
              : tipText}
          </span>
        </div>
      )}
    </div>
  );
}

function FragmentRow({
  label,
  r,
  columns,
  onHover,
}: {
  label: string;
  r: number;
  columns: Cell[][];
  onHover: (c: Cell, el: HTMLElement) => void;
}) {
  return (
    <>
      <span className="heat-day">{label}</span>
      {columns.map((col, c) => {
        const cell = col[r];
        return (
          <i
            key={c}
            className={`heat-cell ${cell.kind} ${cell.kind === "level" ? `lv${cell.level}` : ""}`}
            onMouseEnter={(e) => onHover(cell, e.currentTarget)}
          />
        );
      })}
    </>
  );
}
