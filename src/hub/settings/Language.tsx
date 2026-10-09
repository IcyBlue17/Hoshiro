import { useMemo, useRef, useState } from "react";
import { Check, Plus, Search, X } from "lucide-react";
import { useApp } from "../store";
import { LANGUAGES, languageLabel } from "../format";
import { Popover, Switch, Tooltip } from "../ui";
import { Group, Row } from "./shared";

function LanguagePicker({
  selected,
  onToggle,
  max,
  label,
  disabledTip,
}: {
  selected: string[];
  onToggle: (code: string) => void;
  max?: number;
  label: string;
  disabledTip?: string;
}) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const ref = useRef<HTMLButtonElement>(null);
  const full = max !== undefined && selected.length >= max;
  const list = useMemo(() => {
    const s = q.trim().toLowerCase();
    return LANGUAGES.filter((l) => !s || l.label.toLowerCase().includes(s) || l.code.toLowerCase().includes(s));
  }, [q]);
  return (
    <>
      <Tooltip label={disabledTip ?? ""} delay={full && disabledTip ? 250 : 99999}>
        <button
          ref={ref}
          className="btn btn-ghost btn-sm"
          onClick={() => !full && setOpen((o) => !o)}
          style={full ? { opacity: 0.5, cursor: "not-allowed" } : undefined}
        >
          <Plus size={14} /> {label}
        </button>
      </Tooltip>
      <Popover open={open} onClose={() => setOpen(false)} anchorRef={ref} minWidth={280}>
        <div className="lang-pop">
          <div className="search sm">
            <Search size={14} />
            <input autoFocus value={q} onChange={(e) => setQ(e.target.value)} placeholder="搜索语言" />
          </div>
          <div className="menu-list lang-list">
            {list.length === 0 && <div className="menu-note">没有选项</div>}
            {list.map((l) => {
              const on = selected.includes(l.code);
              const blocked = !on && full;
              return (
                <button
                  key={l.code}
                  className={`menu-item ${on ? "selected" : ""}`}
                  disabled={blocked}
                  onClick={() => onToggle(l.code)}
                >
                  <span className="menu-item-label">{l.label}</span>
                  <span className="menu-item-hint">{l.code}</span>
                  {on && <Check size={15} />}
                </button>
              );
            })}
          </div>
        </div>
      </Popover>
    </>
  );
}

export function LanguageSettings() {
  const { settings, update } = useApp();
  const sel = settings.selectedLanguages;
  const targets = settings.translationTargets;

  return (
    <>
      <Group>
        <Row title="听写语言" desc="选择您在口述中使用的语言及其首选变体，以获得最佳转录效果。" stacked>
          <div className="chip-line">
            {sel.map((c) => (
              <span className="chip removable" key={c}>
                {languageLabel(c)}
                {sel.length > 1 && (
                  <button aria-label="移除" onClick={() => update((d) => void (d.selectedLanguages = d.selectedLanguages.filter((x) => x !== c)))}>
                    <X size={12} />
                  </button>
                )}
              </span>
            ))}
            <LanguagePicker
              label="添加语言"
              selected={sel}
              onToggle={(code) =>
                update((d) => {
                  if (d.selectedLanguages.includes(code)) {
                    if (d.selectedLanguages.length > 1) d.selectedLanguages = d.selectedLanguages.filter((x) => x !== code);
                  } else d.selectedLanguages.push(code);
                })
              }
            />
          </div>
        </Row>
        <Row title="自动检测语言" desc="由语音识别自动判断所说的语言，可混合使用多种语言。">
          <Switch checked={settings.autoSelectLanguages} onChange={(v) => update((d) => void (d.autoSelectLanguages = v))} label="自动检测语言" />
        </Row>
      </Group>

      <Group>
        <Row title="翻译目标" desc="在翻译模式中最多选择 3 种目标语言；第一个为当前使用的目标。" stacked>
          <div className="chip-line">
            {targets.map((c, i) => (
              <span className={`chip removable ${i === 0 ? "primary" : ""}`} key={c}>
                <button
                  className="chip-label"
                  disabled={i === 0}
                  title={i === 0 ? "当前目标语言" : "设为当前目标语言"}
                  onClick={() =>
                    update((d) => {
                      d.translationTargets = [c, ...d.translationTargets.filter((x) => x !== c)];
                    })
                  }
                >
                  {languageLabel(c)}
                </button>
                {i === 0 && <em>当前</em>}
                {targets.length > 1 && (
                  <button aria-label="移除" onClick={() => update((d) => void (d.translationTargets = d.translationTargets.filter((x) => x !== c)))}>
                    <X size={12} />
                  </button>
                )}
              </span>
            ))}
            <LanguagePicker
              label="添加另一种语言"
              max={3}
              disabledTip="最多支持 3 种目标语言。删除一个以添加另一个。"
              selected={targets}
              onToggle={(code) =>
                update((d) => {
                  if (d.translationTargets.includes(code)) {
                    if (d.translationTargets.length > 1) d.translationTargets = d.translationTargets.filter((x) => x !== code);
                  } else if (d.translationTargets.length < 3) d.translationTargets.push(code);
                })
              }
            />
          </div>
        </Row>
      </Group>
    </>
  );
}
