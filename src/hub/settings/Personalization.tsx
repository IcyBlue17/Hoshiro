import { useEffect, useRef, useState } from "react";
import { Sparkles, X } from "lucide-react";
import { api } from "../../lib/ipc";
import { EVENTS, type PersonalProfile } from "../../lib/types";
import { useApp } from "../store";
import { useBackendEvent } from "../hooks";
import { useFeedback } from "../feedback";
import { Spinner, Switch } from "../ui";
import { Group, Row } from "./shared";

const date = (value: string | null) =>
  value ? new Date(value).toLocaleString("zh-CN") : "尚未分析";

export function Personalization() {
  const { settings, update } = useApp();
  const { toast } = useFeedback();
  const [profile, setProfile] = useState<PersonalProfile | null>(null);
  const [draft, setDraft] = useState("");
  const dirty = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
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
  return (
    <>
      <Group>
        <Row
          title="AI 个性化学习"
          desc="将近期口述、翻译历史和您对输入文字的修改发送给已配置的改写模型，学习常用词与表达习惯。关闭后停止学习和应用个人偏好。"
        >
          <Switch
            checked={settings.style.aiPersonalization}
            onChange={(v) =>
              update((d) => void (d.style.aiPersonalization = v))
            }
            label="AI 个性化学习"
          />
        </Row>
        {error && (
          <p className="srow-desc" role="alert">
            {error}
          </p>
        )}
        {!profile && !error && <Spinner />}
        {profile && (
          <Row
            title={
              profile.running ? "正在学习您的表达习惯…" : "越用，越懂您的表达"
            }
            stacked
          >
            <div className="profile-status">
              <span>上次分析：{date(profile.updatedAt)}</span>
              <span>
                上次分析 {profile.analyzedSessions} 条 · 待分析{" "}
                {profile.pendingSessions} 条
              </span>
              <span>
                下次检查：
                {settings.style.aiPersonalization && profile.nextRunAt
                  ? date(profile.nextRunAt)
                  : settings.style.aiPersonalization
                    ? "积累足够记录后自动分析"
                    : "已关闭自动分析"}
              </span>
              <span>
                首次自动分析需 10 条，之后每批 15 条；手动分析需 3 条新记录。
              </span>
            </div>
            {profile.lastError && (
              <p className="srow-desc" role="alert">
                上次分析未完成：{profile.lastError}
              </p>
            )}
            <button
              className="btn btn-secondary"
              disabled={
                running ||
                !settings.style.aiPersonalization ||
                profile.pendingSessions < 3 ||
                !settings.llm[settings.llm.provider].apiKey.trim()
              }
              onClick={run}
            >
              {running ? <Spinner size={14} /> : <Sparkles size={15} />}{" "}
              {running ? "分析中…" : "立即分析"}
            </button>
          </Row>
        )}
      </Group>
      {profile && (
        <>
          <Group title="已学习的习惯">
            {(
              [
                ["verbalTics", "口癖"],
                ["corrections", "常见纠错"],
                ["vocabulary", "专有名词"],
              ] as const
            ).map(([key, label]) => (
              <Row key={key} title={label} stacked>
                <div className="profile-chips">
                  {profile[key].length === 0 && (
                    <span className="muted">还没有学习到内容</span>
                  )}
                  {profile[key].map((item, i) => {
                    const text =
                      typeof item === "string"
                        ? item
                        : `${item.wrong} → ${item.right}`;
                    return (
                      <span className="profile-chip" key={text}>
                        {text}
                        <button
                          className="icon-btn"
                          aria-label={`删除${text}`}
                          disabled={running}
                          onClick={() =>
                            save({
                              ...profile,
                              [key]: profile[key].filter((_, n) => n !== i),
                            })
                          }
                        >
                          <X size={12} />
                        </button>
                      </span>
                    );
                  })}
                </div>
              </Row>
            ))}
            <Row title="风格观察" stacked>
              {profile.styleNotes.length ? (
                <ul className="profile-notes">
                  {profile.styleNotes.map((note) => (
                    <li key={note}>{note}</li>
                  ))}
                </ul>
              ) : (
                <span className="muted">
                  积累一些记录后，这里会显示您的表达偏好。
                </span>
              )}
            </Row>
          </Group>
          <Group title="个人提示词">
            <Row
              title={
                profile.addendumEditedByUser ? "由您维护" : "由 AI 自动更新"
              }
              desc="仅影响用词、语气和格式。编辑并保存后，AI 不会覆盖您的内容。"
              stacked
            >
              <textarea
                className="textarea"
                aria-label="个人提示词"
                rows={5}
                maxLength={1200}
                value={draft}
                onChange={(e) => {
                  dirty.current = true;
                  setDraft(e.target.value);
                }}
                disabled={running}
              />
              <div className="profile-actions">
                <button
                  className="btn btn-secondary"
                  disabled={running || draft === profile.promptAddendum}
                  onClick={() => save({ ...profile, promptAddendum: draft })}
                >
                  保存提示词
                </button>
                {profile.addendumEditedByUser && (
                  <button
                    className="link-btn"
                    disabled={running}
                    onClick={() =>
                      save({ ...profile, addendumEditedByUser: false })
                    }
                  >
                    恢复 AI 自动更新
                  </button>
                )}
              </div>
            </Row>
            <Row title="清空学习结果" desc="保留词典；之后会从历史中重新学习。">
              <button
                className="btn btn-secondary"
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
                重置学习
              </button>
            </Row>
          </Group>
        </>
      )}
    </>
  );
}
