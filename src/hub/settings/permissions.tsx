import { useCallback, useState } from "react";
import { Check } from "lucide-react";
import { api } from "../../lib/ipc";
import { EVENTS, type PermissionKind, type Permissions } from "../../lib/types";
import { useBackendEvent, usePoll } from "../hooks";
import { Row } from "./shared";

export const PERMISSION_INFO: { kind: PermissionKind; title: string; desc: string }[] = [
  { kind: "microphone", title: "麦克风", desc: "仅在您按下快捷键口述时才会使用麦克风录音。" },
  { kind: "accessibility", title: "辅助功能", desc: "用于读取当前应用的上下文，并将文字粘贴到任何文本框中。" },
  { kind: "inputMonitoring", title: "输入监控", desc: "用于识别 Fn 等全局快捷键。" },
];

export function usePermissions(active = true) {
  const [perms, setPerms] = useState<Permissions | null>(null);
  const refresh = useCallback(() => {
    api.getPermissions().then(setPerms).catch(() => {});
  }, []);
  usePoll(refresh, 2000, active);
  useBackendEvent<Permissions>(EVENTS.permissionsChanged, (p) => p && setPerms(p));
  return { perms, setPerms, refresh };
}

export function PermissionAction({
  kind,
  state,
  onChange,
}: {
  kind: PermissionKind;
  state: Permissions[PermissionKind];
  onChange: (p: Permissions) => void;
}) {
  if (state === "granted")
    return (
      <span className="perm-ok">
        <Check size={15} /> 已授权
      </span>
    );
  if (state === "denied")
    return (
      <button className="btn btn-secondary btn-sm" onClick={() => api.openPermissionSettings(kind)}>
        打开系统设置
      </button>
    );
  return (
    <button
      className="btn btn-primary btn-sm"
      onClick={async () => {
        const p = await api.requestPermission(kind);
        onChange(p);
        if (p[kind] !== "granted" && kind !== "microphone") api.openPermissionSettings(kind);
      }}
    >
      授权
    </button>
  );
}

export function PermissionRows() {
  const { perms, setPerms } = usePermissions(true);
  return (
    <>
      {PERMISSION_INFO.map((p) => {
        const st = perms?.[p.kind];
        return (
          <Row
            key={p.kind}
            title={
              <span className="perm-title">
                <i className={`dot ${st === "granted" ? "ok" : st === "denied" ? "bad" : st ? "warn" : "off"}`} />
                {p.title}
              </span>
            }
            desc={p.desc}
          >
            {st && <PermissionAction kind={p.kind} state={st} onChange={setPerms} />}
          </Row>
        );
      })}
    </>
  );
}
