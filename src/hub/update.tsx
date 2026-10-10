import { createContext, type ReactNode, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../lib/ipc";
import { EVENTS, type UpdateChecked, type UpdateInfo, type UpdateProgress } from "../lib/types";
import { useBackendEvent } from "./hooks";
import { useFeedback } from "./feedback";

export type UpdatePhase = "idle" | "checking" | "latest" | "available" | "installing";

interface UpdateCtx {
  phase: UpdatePhase;
  info: UpdateInfo | null;
  /** 0..1, or null while the size is unknown. */
  progress: number | null;
  check: () => Promise<void>;
  install: () => Promise<void>;
}

const Ctx = createContext<UpdateCtx | null>(null);

export function useUpdate(): UpdateCtx {
  const v = useContext(Ctx);
  if (!v) throw new Error("UpdateProvider missing");
  return v;
}

/** One shared update state for About, Home and the sidebar pill. */
export function UpdateProvider({ children }: { children: ReactNode }) {
  const { toast } = useFeedback();
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<number | null>(null);
  const busy = useRef(false);

  useEffect(() => {
    api.getUpdateState().then((i) => i && setInfo(i)).catch(() => {});
  }, []);

  useBackendEvent<UpdateChecked>(EVENTS.updateChecked, (e) => {
    if (e.info) setInfo(e.info);
  });
  useBackendEvent<UpdateInfo>(EVENTS.updateAvailable, (i) => setInfo(i));
  useBackendEvent<UpdateProgress>(EVENTS.updateProgress, (p) => {
    setInstalling(true);
    setProgress(p.total ? Math.min(1, p.downloaded / p.total) : null);
  });

  const check = useCallback(async () => {
    if (busy.current) return;
    busy.current = true;
    setChecking(true);
    try {
      setInfo(await api.checkUpdate());
    } catch (e) {
      toast(typeof e === "string" ? e : "无法检查更新，请稍后再试", "error");
    } finally {
      busy.current = false;
      setChecking(false);
    }
  }, [toast]);

  const install = useCallback(async () => {
    if (busy.current) return;
    busy.current = true;
    setInstalling(true);
    setProgress(null);
    try {
      await api.installUpdate(); // relaunches on success
    } catch (e) {
      toast(typeof e === "string" ? e : "更新失败，请稍后再试", "error");
      setInstalling(false);
    } finally {
      busy.current = false;
    }
  }, [toast]);

  const phase: UpdatePhase = installing ? "installing" : checking ? "checking" : info?.available ? "available" : info ? "latest" : "idle";
  const value = useMemo(() => ({ phase, info, progress, check, install }), [phase, info, progress, check, install]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

/** Short status line shared by the About row and Home link. */
export function updateLabel(phase: UpdatePhase, info: UpdateInfo | null, progress: number | null): string {
  switch (phase) {
    case "checking":
      return "检查中…";
    case "latest":
      return "已是最新版本";
    case "available":
      return `发现新版本 ${info?.version ?? ""}`;
    case "installing":
      return progress !== null && progress >= 1 ? "正在重启…" : "正在下载…";
    default:
      return "";
  }
}
