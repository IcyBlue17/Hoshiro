// Development-only control panel (browser, `?window=bar&demo=1`). Drives the bar through mockEmit.
import { useEffect, useRef, useState } from "react";
import { mockEmit } from "../lib/mock";
import { EVENTS, type BarAlert, type BarState, type HitRect } from "../lib/types";

const base: BarState = {
  phase: "hidden", mode: "dictation", locked: false, translationTargetLabel: null,
  elapsedSecs: 0, showIdleOnDesktop: true, dictationKeyLabel: "fn",
};
const emitState = (p: Partial<BarState>) => mockEmit(EVENTS.barState, { ...base, ...p });
const emitAlert = (a: BarAlert) => mockEmit(EVENTS.barAlert, a);

const LANGS = ["English", "日本語", "Français"];

const LONG_ANSWER = `**Rust** 的所有权系统有三条核心规则：

1. 每个值都有一个**所有者**
2. 同一时间只能有一个所有者
3. 所有者离开作用域时，值会被丢弃

常见的借用规则：

- 任意数量的不可变引用 \`&T\`
- 或者 *一个* 可变引用 \`&mut T\`
- 引用必须始终有效

示例代码：

\`\`\`rust
fn main() {
    let s = String::from("hello");
    let r = &s;
    println!("{r}");
}
\`\`\`

> 编译器会在编译期检查这些规则，所以没有运行时开销。

更多内容请参考 [The Rust Book](https://doc.rust-lang.org/book/)。最后再补充一点：生命周期标注只是告诉编译器引用之间的关系，并不会改变引用实际存活的时间。`;

const SHORT_ANSWER = "东京今天大约 18°C，多云，傍晚可能有小雨，建议带一把伞。";

export default function Demo() {
  const [open, setOpen] = useState(true);
  const [showRects, setShowRects] = useState(false);
  const [rects, setRects] = useState<HitRect[]>([]);
  const speech = useRef<number>(0);
  const langIdx = useRef(0);
  const finishTimer = useRef<number>(0);

  const stopSpeech = () => { clearInterval(speech.current); speech.current = 0; mockEmit(EVENTS.barLevel, 0); };
  const startSpeech = () => {
    stopSpeech();
    let speaking = true, until = performance.now() + 1500, amp = 0.5, t0 = performance.now();
    speech.current = window.setInterval(() => {
      const now = performance.now();
      if (now > until) {
        speaking = !speaking;
        until = now + (speaking ? 700 + Math.random() * 1600 : 400 + Math.random() * 700);
        amp = 0.25 + Math.random() * 0.6;
      }
      let v = 0.004 + Math.random() * 0.004;
      if (speaking) {
        const syll = Math.abs(Math.sin(((now - t0) / 1000) * Math.PI * 4.2));
        v = amp * Math.pow(syll, 0.7) * (0.75 + Math.random() * 0.25) + Math.random() * 0.04;
      }
      mockEmit(EVENTS.barLevel, Math.min(1, v));
    }, 33);
  };

  const run = (fn: () => void) => () => { clearTimeout(finishTimer.current); fn(); };
  const hide = () => { stopSpeech(); emitState({ phase: "hidden" }); };
  const process = (extra: Partial<BarState> = {}) => {
    stopSpeech();
    emitState({ phase: "processing", ...extra });
    finishTimer.current = window.setTimeout(() => emitState({ phase: "hidden" }), 3500);
  };

  // Simulate the backend's reaction to bar buttons (they carry data-act).
  useEffect(() => {
    const onClick = (e: MouseEvent) => {
      const el = (e.target as HTMLElement).closest<HTMLElement>("[data-act]");
      if (!el) return;
      const a = el.dataset.act;
      if (a === "start") { emitState({ phase: "recording", locked: true }); startSpeech(); }
      else if (a === "cancel") {
        hide();
        emitAlert({ id: "cancelled", kind: "transcript_cancelled", title: "转录已取消", description: null, tone: "info", durationMs: 5000,
          actions: [{ id: "undo", label: "撤销", style: "primary" }] });
      } else if (a === "finish") process();
      else if (a === "cycle_translation_target") {
        langIdx.current = (langIdx.current + 1) % LANGS.length;
        emitState({ phase: "recording", mode: "translation", locked: true, translationTargetLabel: LANGS[langIdx.current] });
      }
    };
    document.addEventListener("click", onClick, true);
    return () => { document.removeEventListener("click", onClick, true); stopSpeech(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!showRects) return;
    const id = window.setInterval(() => setRects(((window as unknown as { __hitRects?: HitRect[] }).__hitRects ?? []).slice()), 100);
    return () => clearInterval(id);
  }, [showRects]);

  const alertBtn = (label: string, a: BarAlert) => <button key={label} onClick={() => emitAlert(a)}>{label}</button>;

  return (
    <>
      <div className="demo-desktop" />
      {showRects && rects.map((r, i) => (
        <div key={i} className="demo-rect" style={{ left: r.x, top: r.y, width: r.width, height: r.height }} />
      ))}
      <div className="demo-panel">
        <button className="demo-toggle" onClick={() => setOpen((v) => !v)}>{open ? "demo ▾" : "demo ▸"}</button>
        {open && (
          <>
            <div className="demo-group">
              <b>state</b>
              <button onClick={run(hide)}>hidden</button>
              <button onClick={run(() => { stopSpeech(); emitState({ phase: "idle" }); })}>idle</button>
              <button onClick={run(() => { emitState({ phase: "recording" }); startSpeech(); })}>PTT</button>
              <button onClick={run(() => { emitState({ phase: "recording", locked: true }); startSpeech(); })}>locked</button>
              <button onClick={run(() => { emitState({ phase: "recording", locked: true, elapsedSecs: 64 }); startSpeech(); })}>locked 1:04</button>
              <button onClick={run(() => { emitState({ phase: "recording", mode: "translation", locked: true, translationTargetLabel: LANGS[langIdx.current] }); startSpeech(); })}>translate</button>
              <button onClick={run(() => { emitState({ phase: "recording", mode: "ask", locked: true }); startSpeech(); })}>ask</button>
              <button onClick={run(() => process())}>processing</button>
              <button onClick={run(() => process({ mode: "ask", locked: false }))}>proc ask</button>
              <button onClick={run(() => { emitState({ phase: "recording" }); startSpeech(); setTimeout(() => process(), 2500); })}>PTT→proc→hide</button>
              <button onClick={run(() => { emitState({ phase: "idle", showIdleOnDesktop: false }); })}>idle off</button>
              <button onClick={() => { emitState({ phase: "recording" }); mockEmit(EVENTS.barLevel, 0); stopSpeech(); }}>silence</button>
            </div>
            <div className="demo-group">
              <b>alerts</b>
              {alertBtn("取消+撤销", { id: "cancelled", kind: "transcript_cancelled", title: "转录已取消", description: null, tone: "info", durationMs: 5000, actions: [{ id: "undo", label: "撤销", style: "primary" }] })}
              {alertBtn("出现问题", { id: "error", kind: "transcription_error", title: "出现问题", description: "无法完成写作。点击重试继续。", tone: "error", durationMs: null, actions: [{ id: "retry", label: "重试", style: "primary" }, { id: "open_history", label: "打开历史记录", style: "secondary" }] })}
              {alertBtn("复制最后", { id: "copy", kind: "copy_last", title: "复制最后的转录", description: null, tone: "info", durationMs: 8000, actions: [{ id: "copy", label: "复制", style: "primary" }] })}
              {alertBtn("hint", { id: "hint", kind: "hint", title: "按住 [[fn]] 说话，双击 [[fn]] 进入免提模式", description: null, tone: "info", durationMs: 3000, actions: [] })}
              {alertBtn("词典", { id: "dict" + Date.now(), kind: "dictionary_added", title: "\"Hoshiro\" 已添加到您的词典", description: null, tone: "success", durationMs: 4000, actions: [] })}
              {alertBtn("网络慢", { id: "slow", kind: "no_network", title: "连接比平常慢…", description: "网络连接不佳，仍在处理您的转录。", tone: "warning", durationMs: null, actions: [{ id: "cancel", label: "取消", style: "secondary" }] })}
              {alertBtn("仍在处理", { id: "pending", kind: "previous_pending", title: "Hoshiro 仍在处理您的上一个转录", description: "如果您想取消上一个转录，请按*Esc*或点击下面。", tone: "warning", durationMs: 6000, actions: [{ id: "cancel", label: "取消", style: "danger" }] })}
              <button onClick={() => mockEmit(EVENTS.barAlertDismiss, "error")}>dismiss error</button>
            </div>
            <div className="demo-group">
              <b>answer</b>
              <button onClick={() => mockEmit(EVENTS.barAnswer, { id: "a1", question: "Rust 的所有权系统是怎么工作的？请简单解释一下借用规则。", answer: LONG_ANSWER })}>long md</button>
              <button onClick={() => mockEmit(EVENTS.barAnswer, { id: "a2", question: "东京今天天气怎么样？", answer: SHORT_ANSWER })}>short</button>
              <label><input type="checkbox" checked={showRects} onChange={(e) => setShowRects(e.target.checked)} /> hit rects</label>
            </div>
          </>
        )}
      </div>
    </>
  );
}
