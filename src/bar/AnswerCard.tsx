import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { motion } from "motion/react";
import { Check, Copy, X } from "lucide-react";
import { api } from "../lib/ipc";
import type { AskAnswer } from "../lib/types";
import { Markdown } from "./Markdown";

const LINE_PX = 13 * 1.55; // keep in sync with .answer-body line-height
const COLLAPSED_LINES = 8;
const COLLAPSED_H = Math.round(LINE_PX * COLLAPSED_LINES);
const EXPANDED_MAX_H = 190;

export function AnswerCard({ answer, onClose }: { answer: AskAnswer; onClose: (id: string) => void }) {
  const innerRef = useRef<HTMLDivElement>(null);
  const [contentH, setContentH] = useState(0);
  const [expanded, setExpanded] = useState(false);
  const [copied, setCopied] = useState(false);
  const copyTimer = useRef<number>(0);

  useLayoutEffect(() => {
    const el = innerRef.current!;
    const ro = new ResizeObserver(() => setContentH(el.offsetHeight));
    ro.observe(el);
    setContentH(el.offsetHeight);
    return () => ro.disconnect();
  }, []);
  useEffect(() => { setExpanded(false); setCopied(false); }, [answer.id]);
  useEffect(() => () => clearTimeout(copyTimer.current), []);

  const overflowing = contentH > COLLAPSED_H + 4;
  const target = overflowing ? (expanded ? Math.min(contentH, EXPANDED_MAX_H) : COLLAPSED_H) : contentH;

  const copy = () => {
    void api.copyText(answer.answer);
    setCopied(true);
    clearTimeout(copyTimer.current);
    copyTimer.current = window.setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="answer-card" data-hit>
      <div className="answer-head">
        <span className="answer-title">回答</span>
        <span className="answer-spacer" />
        <button className={`icon-btn${copied ? " ok" : ""}`} onClick={copy} title={copied ? "已复制" : "复制"} aria-label="复制">
          {copied ? <Check size={14} strokeWidth={2.4} /> : <Copy size={14} strokeWidth={2} />}
          {copied && <span className="icon-btn-label">已复制</span>}
        </button>
        <button className="icon-btn" onClick={() => onClose(answer.id)} title="关闭" aria-label="关闭">
          <X size={15} strokeWidth={2.2} />
        </button>
      </div>
      {answer.question && <div className="answer-question">{answer.question}</div>}
      <motion.div
        className={`answer-body${overflowing && !expanded ? " faded" : ""}${overflowing && expanded ? " scroll" : ""}`}
        initial={false}
        animate={{ height: target }}
        transition={{ type: "spring", stiffness: 420, damping: 40 }}
      >
        <div ref={innerRef} className="answer-inner"><Markdown text={answer.answer} /></div>
      </motion.div>
      {overflowing && (
        <button className="answer-more" onClick={() => setExpanded((v) => !v)}>
          {expanded ? "显示更少" : "...更多"}
        </button>
      )}
    </div>
  );
}
