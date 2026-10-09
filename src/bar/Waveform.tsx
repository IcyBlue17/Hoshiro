import { useEffect, useRef, type MutableRefObject } from "react";

/** Latest mic level pushed by the backend (~30 Hz). Lives in a ref: never causes a React render. */
export type LevelRef = MutableRefObject<{ v: number; t: number }>;
export type WaveMode = "live" | "processing" | "idle";

const N = 11;
const BAR_W = 3;
const GAP = 3;
const W = N * BAR_W + (N - 1) * GAP; // 63
const H = 24;
const MIN_H = 3;
const MAX_H = 20;
const LERP = 0.3; // per 60 Hz frame

/**
 * Canvas waveform driven by requestAnimationFrame. Heights follow the live level with a centre-weighted
 * envelope, smooth per-bar variation and a tiny idle "breathing"; in `processing` the bars collapse into
 * dots that pulse left to right.
 */
export function Waveform({ levelRef, mode, color = "255,255,255" }: { levelRef: LevelRef; mode: WaveMode; color?: string }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const modeRef = useRef(mode);
  const colorRef = useRef(color);
  modeRef.current = mode;
  colorRef.current = color;

  useEffect(() => {
    const canvas = canvasRef.current!;
    const ctx = canvas.getContext("2d")!;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)");
    const heights = new Float32Array(N).fill(MIN_H);
    const f1 = Array.from({ length: N }, () => 4 + Math.random() * 4);
    const f2 = Array.from({ length: N }, () => 9 + Math.random() * 6);
    const p1 = Array.from({ length: N }, () => Math.random() * 6.283);
    const p2 = Array.from({ length: N }, () => Math.random() * 6.283);
    const bell = Array.from({ length: N }, (_, i) => Math.exp(-(((i - (N - 1) / 2) / 3.4) ** 2)));
    let env = 0;
    let dpr = 0;
    let prev = performance.now();
    let raf = 0;

    const fit = () => {
      const d = window.devicePixelRatio || 1;
      if (d === dpr) return;
      dpr = d;
      canvas.width = Math.round(W * d);
      canvas.height = Math.round(H * d);
      ctx.setTransform(d, 0, 0, d, 0, 0);
    };

    const pill = (x: number, y: number, w: number, h: number) => {
      const r = Math.min(w, h) / 2;
      ctx.beginPath();
      ctx.moveTo(x + r, y);
      ctx.arcTo(x + w, y, x + w, y + h, r);
      ctx.arcTo(x + w, y + h, x, y + h, r);
      ctx.arcTo(x, y + h, x, y, r);
      ctx.arcTo(x, y, x + w, y, r);
      ctx.closePath();
    };

    const frame = (now: number) => {
      raf = requestAnimationFrame(frame);
      fit();
      const dt = Math.min(64, now - prev);
      prev = now;
      const k60 = dt / 16.667;
      const m = modeRef.current;
      const t = now / 1000;

      let input = 0;
      if (m === "live") {
        const l = levelRef.current;
        input = now - l.t < 300 ? l.v : 0;
        input = input < 0.01 ? 0 : Math.pow(Math.min(1, input), 0.6);
      }
      const a = input > env ? 0.6 : 0.16;
      env += (input - env) * (1 - Math.pow(1 - a, k60));
      const kLerp = 1 - Math.pow(1 - LERP, k60);
      const still = reduce.matches;
      // 0 when speaking, 1 in silence
      const quiet = 1 - Math.min(1, env * 8);

      ctx.clearRect(0, 0, W, H);
      const rgb = colorRef.current;
      for (let i = 0; i < N; i++) {
        const w = 0.3 + 0.7 * bell[i];
        let target = MIN_H;
        let alpha = 1;
        if (m === "live") {
          const noise = 0.5 + 0.25 * Math.sin(t * f1[i] + p1[i]) + 0.25 * Math.sin(t * f2[i] + p2[i]);
          const speech = Math.min(1, env * w * (0.58 + 0.42 * noise) * 1.3);
          const breath = still ? 0 : quiet * (0.5 + 0.5 * Math.sin(t * 2.4 - i * 0.55)) * (0.35 + 0.65 * bell[i]) * 2.4;
          target = MIN_H + (MAX_H - MIN_H) * speech + breath;
        } else if (m === "processing") {
          alpha = still ? 0.8 : 0.4 + 0.6 * (0.5 + 0.5 * Math.sin(t * 5.5 - i * 0.65));
        } else {
          alpha = 0.6;
        }
        heights[i] += (target - heights[i]) * kLerp;
        const h = heights[i];
        ctx.fillStyle = `rgba(${rgb},${alpha.toFixed(3)})`;
        pill(i * (BAR_W + GAP), (H - h) / 2, BAR_W, h);
        ctx.fill();
      }
    };

    fit();
    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  }, [levelRef]);

  return <canvas ref={canvasRef} className="waveform" style={{ width: W, height: H }} />;
}
