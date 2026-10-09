import { useEffect, useRef } from "react";
import { listen } from "../lib/ipc";

/** Subscribe to a backend event for the lifetime of the component. */
export function useBackendEvent<T = unknown>(event: string, handler: (payload: T) => void) {
  const ref = useRef(handler);
  ref.current = handler;
  useEffect(() => {
    let un: (() => void) | undefined;
    let alive = true;
    listen<T>(event, (p) => ref.current(p)).then((u) => {
      if (alive) un = u;
      else u();
    });
    return () => {
      alive = false;
      un?.();
    };
  }, [event]);
}

/** Calls `fn` now and then every `ms` while `active`. */
export function usePoll(fn: () => void, ms: number, active = true) {
  const ref = useRef(fn);
  ref.current = fn;
  useEffect(() => {
    if (!active) return;
    ref.current();
    const t = window.setInterval(() => ref.current(), ms);
    return () => window.clearInterval(t);
  }, [ms, active]);
}
