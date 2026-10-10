import { useEffect, type RefObject } from "react";
import { api, isTauri } from "../lib/ipc";
import type { HitRect } from "../lib/types";

const MIN_INTERVAL = 34; // ≤ ~30 calls/s
const SETTLE_MS = 700; // keep sampling this long after the last DOM/animation activity

/**
 * Reports every `[data-hit]` element's box (CSS px, window-relative) to the backend so only those regions
 * receive mouse events. Optional attributes:
 *   data-hit-pad="x,y"   inflate the rect (tiny idle pill)
 *   data-hit-clip        on an ancestor: intersect with that ancestor's box (collapsing cards)
 * Sampling is event-driven (Mutation/Resize observers, transition/animation end) plus a short rAF poll after any
 * activity, because transforms/springs don't trigger observers. Only changed lists are sent.
 */
export function useHitRects(rootRef: RefObject<HTMLElement | null>) {
  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    let last = "";
    let lastSent = -Infinity;
    let raf = 0;
    let until = 0;
    let lastSample = 0;
    let trailing = 0;
    let first = true;

    const measure = (): HitRect[] => {
      const out: HitRect[] = [];
      root.querySelectorAll<HTMLElement>("[data-hit]").forEach((el) => {
        let r = el.getBoundingClientRect();
        let x0 = r.left, y0 = r.top, x1 = r.right, y1 = r.bottom;
        const clip = el.closest<HTMLElement>("[data-hit-clip]");
        if (clip && clip !== el) {
          const c = clip.getBoundingClientRect();
          x0 = Math.max(x0, c.left); y0 = Math.max(y0, c.top); x1 = Math.min(x1, c.right); y1 = Math.min(y1, c.bottom);
        }
        const pad = el.dataset.hitPad?.split(",").map(Number);
        if (pad) { const px = pad[0] || 0, py = pad[1] ?? px; x0 -= px; x1 += px; y0 -= py; y1 += py; }
        x0 = Math.max(0, Math.floor(x0)); y0 = Math.max(0, Math.floor(y0));
        x1 = Math.min(window.innerWidth, Math.ceil(x1)); y1 = Math.min(window.innerHeight, Math.ceil(y1));
        if (x1 - x0 < 1 || y1 - y0 < 1) return;
        if (getComputedStyle(el).opacity === "0") return;
        out.push({ x: x0, y: y0, width: x1 - x0, height: y1 - y0 });
      });
      return out;
    };

    const send = (json: string, rects: HitRect[]) => {
      last = json;
      lastSent = performance.now();
      first = false;
      if (!isTauri) (window as unknown as { __hitRects?: HitRect[] }).__hitRects = rects;
      void api.bar.setHitRects(rects);
    };

    const flush = () => {
      const rects = measure();
      const json = JSON.stringify(rects);
      if (json === last && !first) return;
      const wait = MIN_INTERVAL - (performance.now() - lastSent);
      if (wait <= 0) {
        send(json, rects);
      } else if (!trailing) {
        trailing = window.setTimeout(() => { trailing = 0; flush(); }, wait);
      }
    };

    const loop = (now: number) => {
      if (now - lastSample >= 30) { lastSample = now; flush(); }
      if (now < until) raf = requestAnimationFrame(loop);
      else { raf = 0; flush(); }
    };

    const kick = () => {
      until = performance.now() + SETTLE_MS;
      if (!raf) raf = requestAnimationFrame(loop);
    };

    const mo = new MutationObserver(kick);
    mo.observe(root, { subtree: true, childList: true, attributes: true, attributeFilter: ["data-hit", "class", "style", "data-hit-pad"] });
    const ro = new ResizeObserver(kick);
    ro.observe(root);
    root.addEventListener("transitionend", kick, true);
    root.addEventListener("animationend", kick, true);
    window.addEventListener("resize", kick);

    flush(); // once on mount
    kick();

    return () => {
      mo.disconnect();
      ro.disconnect();
      root.removeEventListener("transitionend", kick, true);
      root.removeEventListener("animationend", kick, true);
      window.removeEventListener("resize", kick);
      cancelAnimationFrame(raf);
      clearTimeout(trailing);
    };
  }, [rootRef]);
}
