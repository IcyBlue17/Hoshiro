import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { api, listen } from "../lib/ipc";
import { EVENTS, type Settings } from "../lib/types";

export type Page = "home" | "history" | "dictionary";
export type SettingsCategory =
  | "general"
  | "shortcuts"
  | "audio"
  | "language"
  | "providers"
  | "style"
  | "data"
  | "permissions"
  | "about";

interface AppCtx {
  settings: Settings;
  /** Apply a mutation and persist it immediately (optimistic). */
  update: (fn: (draft: Settings) => void) => void;
  openSettings: (category?: SettingsCategory) => void;
  navigate: (page: Page) => void;
  version: string;
}

export const AppContext = createContext<AppCtx | null>(null);

export function useApp(): AppCtx {
  const v = useContext(AppContext);
  if (!v) throw new Error("AppContext missing");
  return v;
}

/** Owns the Settings object: initial load, live updates from the backend, optimistic saves. */
export function useSettingsStore() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const ref = useRef<Settings | null>(null);
  const seq = useRef(0);

  const apply = useCallback((s: Settings) => {
    ref.current = s;
    setSettings(s);
  }, []);

  useEffect(() => {
    let alive = true;
    api.getSettings().then((s) => alive && apply(s));
    let un: (() => void) | undefined;
    listen<Settings>(EVENTS.settingsUpdated, (s) => {
      if (s && alive) apply(s);
    }).then((u) => {
      if (alive) un = u;
      else u();
    });
    return () => {
      alive = false;
      un?.();
    };
  }, [apply]);

  const update = useCallback(
    (fn: (draft: Settings) => void) => {
      if (!ref.current) return;
      const next = structuredClone(ref.current);
      fn(next);
      apply(next);
      const mine = ++seq.current;
      api
        .saveSettings(next)
        .then((saved) => {
          if (saved && mine === seq.current) apply(saved);
        })
        .catch((e) => console.error("saveSettings failed", e));
    },
    [apply],
  );

  const reload = useCallback(() => api.getSettings().then(apply), [apply]);

  return { settings, update, reload };
}

export function useAppMemo(
  settings: Settings,
  update: AppCtx["update"],
  openSettings: AppCtx["openSettings"],
  navigate: AppCtx["navigate"],
  version: string,
) {
  return useMemo(
    () => ({ settings, update, openSettings, navigate, version }),
    [settings, update, openSettings, navigate, version],
  );
}
