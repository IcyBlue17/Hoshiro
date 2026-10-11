import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { motion } from "motion/react";
import { Check, Copy, X } from "lucide-react";
import { api } from "../lib/ipc";
import type { AskAnswer } from "../lib/types";
import { Markdown } from "./Markdown";

const MAX_H = 190;
const FADE_PX = 24;

// 两端渐隐的深度随离边缘的距离连续变化，直接写 CSS 变量，滚动时不触发重渲染
function updateFades(el: HTMLElement) {
  const below = el.scrollHeight - el.clientHeight - el.scrollTop;
  el.style.setProperty("--fade-top", `${Math.min(Math.max(el.scrollTop, 0), FADE_PX)}px`);
  el.style.setProperty("--fade-bottom", `${Math.min(Math.max(below, 0), FADE_PX)}px`);
}

export function AnswerCard({ answer, onClose }: { answer: AskAnswer; onClose: (id: string) => void }) {
  const innerRef = useRef<HTMLDivElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const [contentH, setContentH] = useState(0);
  const [copied, setCopied] = useState(false);
  const copyTimer = useRef<number>(0);
  const scrollTimer = useRef<number>(0);

  useLayoutEffect(() => {
    const el = innerRef.current!;
    const ro = new ResizeObserver(() => setContentH(el.offsetHeight));
    ro.observe(el);
    setContentH(el.offsetHeight);
    return () => ro.disconnect();
  }, []);
  useEffect(() => { setCopied(false); }, [answer.id]);
  useEffect(() => () => { clearTimeout(copyTimer.current); clearTimeout(scrollTimer.current); }, []);
  useLayoutEffect(() => { if (bodyRef.current) updateFades(bodyRef.current); }, [contentH]);

  const overflowing = contentH > MAX_H;
  const target = Math.min(contentH, MAX_H);

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
        ref={bodyRef}
        className={`answer-body${overflowing ? " scroll" : ""}`}
        onScroll={(e) => {
          const el = e.currentTarget;
          updateFades(el);
          // 滚动条只在滚动时浮现，停下片刻后淡出
          el.classList.add("scrolling");
          clearTimeout(scrollTimer.current);
          scrollTimer.current = window.setTimeout(() => el.classList.remove("scrolling"), 700);
        }}
        initial={false}
        animate={{ height: target }}
        transition={{ type: "spring", stiffness: 420, damping: 40 }}
        onUpdate={() => bodyRef.current && updateFades(bodyRef.current)}
      >
        <div ref={innerRef} className="answer-inner"><Markdown text={answer.answer} /></div>
      </motion.div>
    </div>
  );
}
