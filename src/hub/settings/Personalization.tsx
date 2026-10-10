import { useEffect, useRef, useState } from "react";
import { Sparkles, X } from "lucide-react";
import { api } from "../../lib/ipc";
import { EVENTS, type PersonalProfile } from "../../lib/types";
import { useApp } from "../store";
import { useBackendEvent } from "../hooks";
import { useFeedback } from "../feedback";
import { Segmented, Spinner, Switch } from "../ui";
import { Group, Row } from "./shared";
import { EFFORT_OPTIONS, ModelInput, PROVIDERS } from "./Providers";

const shortDate = (value: string) => {
  const d = new Date(value);
  return `${d.getMonth() + 1}月${d.getDate()}日`;
};

export function Personalization() {
  const { settings, update } = useApp();
  const { toast } = useFeedback();
  const [profile, setProfile] = useState<PersonalProfile | null>(null);
  const [draft, setDraft] = useState("");
  const dirty = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [showAll, setShowAll] = useState(false);
  const apply = (p: PersonalProfile) => {
    dirty.current = false;
    setProfile(p);
    setDraft(p.promptAddendum);
  };
  useEffect(() => {
    api
      .getProfile()
      .then(apply)
      .catch((e) => setError(String(e)));
  }, []);
  useBackendEvent<PersonalProfile>(EVENTS.profileUpdated, (p) => {
    setProfile(p);
    if (!dirty.current) setDraft(p.promptAddendum);
  });
  useBackendEvent(EVENTS.historyUpdated, () =>
    api
      .getProfile()
      .then(setProfile)
      .catch((e) => setError(String(e))),
  );
  const save = async (next: PersonalProfile) => {
    setBusy(true);
    try {
      const saved = await api.updateProfile(next);
      if (dirty.current && next.promptAddendum === profile?.promptAddendum)
        setProfile(saved);
      else apply(saved);
    } catch (e) {
      toast(String(e), "error");
    } finally {
      setBusy(false);
    }
  };
  const running = busy || profile?.running;
  const run = async () => {
    setBusy(true);
    try {
      const saved = await api.runPersonalization();
      if (dirty.current) setProfile(saved);
      else apply(saved);
    } catch (e) {
      toast(String(e), "error");
    } finally {
      setBusy(false);
    }
  };
  const enabled = settings.style.aiPersonalization;
  const learned = profile
    ? [
        ...profile.vocabulary.map((t, i) => ({ key: "vocabulary" as const, i, text: t })),
        ...profile.corrections.map((c, i) => ({ key: "corrections" as const, i, text: `${c.wrong} → ${c.right}` })),
        ...profile.verbalTics.map((t, i) => ({ key: "verbalTics" as const, i, text: t })),
      ]
    : [];
  const shown = showAll ? learned : learned.slice(0, 18);
  const status = !enabled
    ? "已关闭"
    : profile?.running
      ? "正在学习…"
      : profile?.updatedAt
        ? `上次学习 ${shortDate(profile.updatedAt)} · 已学会 ${learned.length} 项`
        : "积累一些记录后自动开始";
  const remove = (key: "vocabulary" | "corrections" | "verbalTics", i: number) => {
    if (!profile) return;
    save({ ...profile, [key]: (profile[key] as unknown[]).filter((_, n) => n !== i) } as PersonalProfile);
  };
  return (
    <>
      <Group>
        <Row title="AI 个性化学习" desc={error || profile?.lastError ? "上次学习未完成" : status}>
          <div className="pz-head">
            {enabled && profile && (
              <button
                className="btn btn-secondary btn-sm"
                disabled={running || profile.pendingSessions < 3 || !settings.llm[settings.llm.provider].apiKey.trim()}
                onClick={run}
              >
                {running ? <Spinner size={13} /> : <Sparkles size={14} />}
                {running ? "学习中" : "立即学习"}
              </button>
            )}
            <Switch checked={enabled} onChange={(v) => update((d) => void (d.style.aiPersonalization = v))} label="AI 个性化学习" />
          </div>
        </Row>
        {enabled && learned.length > 0 && (
          <Row title="已学会" stacked>
            <div className="profile-chips">
              {shown.map((it) => (
                <span className="profile-chip" key={`${it.key}-${it.text}`}>
                  {it.text}
                  <button className="icon-btn" aria-label={`删除${it.text}`} disabled={running} onClick={() => remove(it.key, it.i)}>
                    <X size={12} />
                  </button>
                </span>
              ))}
              {learned.length > shown.length && (
                <button className="link-btn" onClick={() => setShowAll(true)}>
                  全部 {learned.length} 项
                </button>
              )}
            </div>
          </Row>
        )}
      </Group>
      {enabled && profile && (
        <details className="pz-advanced">
          <summary>高级</summary>
          <AnalysisModel />
          <Group>
            <Row title="个人提示词" stacked>
              <textarea
                className="textarea"
                aria-label="个人提示词"
                rows={4}
                maxLength={1200}
                value={draft}
                onChange={(e) => {
                  dirty.current = true;
                  setDraft(e.target.value);
                }}
                disabled={running}
              />
              <div className="profile-actions">
                <button className="btn btn-secondary btn-sm" disabled={running || draft === profile.promptAddendum} onClick={() => save({ ...profile, promptAddendum: draft })}>
                  保存
                </button>
                {profile.addendumEditedByUser && (
                  <button className="link-btn" disabled={running} onClick={() => save({ ...profile, addendumEditedByUser: false })}>
                    恢复自动更新
                  </button>
                )}
                <span className="pz-spacer" />
                <button
                  className="link-btn danger"
                  disabled={running}
                  onClick={async () => {
                    setBusy(true);
                    try {
                      apply(await api.resetProfile());
                    } catch (e) {
                      toast(String(e), "error");
                    } finally {
                      setBusy(false);
                    }
                  }}
                >
                  清空学习结果
                </button>
              </div>
            </Row>
          </Group>
        </details>
      )}
    </>
  );
}

function AnalysisModel() {
  const { settings, update } = useApp();
  const { llm } = settings;
  const p = llm.personalization;
  const follow = p.followRewrite;
  const provider = follow ? llm.provider : p.provider;
  const cfg = llm[provider];
  return (
    <Group>
      <Row title="学习模型" desc={follow ? "与改写相同" : undefined}>
        <Segmented
          id="pz-follow"
          size="sm"
          value={follow ? "follow" : "custom"}
          onChange={(v) => update((d) => void (d.llm.personalization.followRewrite = v === "follow"))}
          options={[
            { value: "follow", label: "跟随改写" },
            { value: "custom", label: "单独指定" },
          ]}
        />
      </Row>
      {!follow && (
        <>
          <Row title="服务商">
            <Segmented
              id="pz-provider"
              size="sm"
              value={p.provider}
              onChange={(v) => update((d) => void (d.llm.personalization.provider = v))}
              options={PROVIDERS.map((x) => ({ value: x.value, label: x.label }))}
            />
          </Row>
          <Row title="模型" desc={cfg.apiKey.trim() ? undefined : "该服务商还没有 API Key"}>
            <ModelInput
              key={provider}
              provider={provider}
              cfg={cfg}
              value={p.model}
              placeholder={cfg.model || "默认"}
              onPick={(m) => update((d) => void (d.llm.personalization.model = m.trim()))}
            />
          </Row>
        </>
      )}
      <Row title="思考强度">
        <Segmented
          id="pz-effort"
          size="sm"
          value={p.reasoningEffort}
          onChange={(v) => update((d) => void (d.llm.personalization.reasoningEffort = v))}
          options={EFFORT_OPTIONS}
        />
      </Row>
    </Group>
  );
}
