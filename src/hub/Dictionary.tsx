import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { BookOpen, FileUp, Info, Pencil, PenLine, Plus, Search, Sparkles, Trash2, X } from "lucide-react";
import { api, isTauri } from "../lib/ipc";
import { EVENTS, type DictionaryWord } from "../lib/types";
import { useBackendEvent } from "./hooks";
import { useFeedback } from "./feedback";
import { Checkbox, Modal, ModalHeader, PageTitle, Segmented, Tooltip } from "./ui";

type Tab = "all" | "auto" | "manual";
const TABS: { value: Tab; label: string }[] = [
  { value: "all", label: "所有" },
  { value: "auto", label: "自动添加" },
  { value: "manual", label: "手动添加" },
];
const EMPTY_DESC: Record<Tab, string> = {
  all: "Hoshiro 会记住您独特的名称和词汇，这些都是通过您的编辑自动学习或您手动添加的。",
  auto: "Hoshiro 会自动学习您独特的名称和词汇，并将其添加到您的个人词典中。",
  manual: "您可以手动将独特的名称和词汇添加到您的个人词典中。",
};
const MAX_LEN = 60;

export function DictionaryPage() {
  const { toast, confirm } = useFeedback();
  const [words, setWords] = useState<DictionaryWord[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [tab, setTab] = useState<Tab>("all");
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [addOpen, setAddOpen] = useState(false);
  const [editing, setEditing] = useState<DictionaryWord | null>(null);
  const [preview, setPreview] = useState<{ fileName: string; words: string[] } | null>(null);
  const [importing, setImporting] = useState(false);

  const load = useCallback(async () => {
    try {
      setWords(await api.listDictionary());
    } finally {
      setLoaded(true);
    }
  }, []);
  useEffect(() => {
    load();
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, [load]);
  useBackendEvent(EVENTS.historyUpdated, load);

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    return words
      .filter((w) => (tab === "all" ? true : w.source === tab))
      .filter((w) => !q || w.word.toLowerCase().includes(q))
      .sort((a, b) => b.createdAt.localeCompare(a.createdAt));
  }, [words, tab, query]);

  useEffect(() => {
    setSelected((s) => {
      const ids = new Set(words.map((w) => w.id));
      const n = new Set([...s].filter((id) => ids.has(id)));
      return n.size === s.size ? s : n;
    });
  }, [words]);

  const toggle = (id: string) =>
    setSelected((s) => {
      const n = new Set(s);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });

  const removeOne = async (w: DictionaryWord) => {
    const ok = await confirm({
      title: "删除词汇？",
      description: `'${w.word}' 将被永久删除，无法恢复。`,
      confirmLabel: "删除",
      danger: true,
    });
    if (!ok) return;
    await api.deleteDictionaryWords([w.id]);
    toast(`'${w.word}' 已删除`);
    load();
  };

  const removeSelected = async () => {
    const n = selected.size;
    const ok = await confirm({
      title: `删除 ${n} 个词？`,
      description: "所选的词将被永久删除，无法恢复。",
      confirmLabel: "删除",
      danger: true,
    });
    if (!ok) return;
    await api.deleteDictionaryWords([...selected]);
    setSelected(new Set());
    toast(`已删除 ${n} 个词`);
    load();
  };

  const importCsv = async () => {
    setImporting(true);
    try {
      let path: string | null = null;
      if (isTauri) {
        const { open } = await import("@tauri-apps/plugin-dialog");
        const p = await open({ multiple: false, directory: false, filters: [{ name: "CSV", extensions: ["csv"] }] });
        path = typeof p === "string" ? p : null;
      } else {
        path = "/Users/me/words.csv";
      }
      if (!path) return;
      const fileName = path.split(/[\\/]/).pop() ?? path;
      const list = await api.previewDictionaryCsv(path);
      if (!list || list.length === 0) toast(`在${fileName}中未找到有效的单词`, "error");
      else setPreview({ fileName, words: list });
    } catch (e) {
      toast(`文件无法导入。请使用一列的 CSV 文件，每行一个单词。`, "error");
      console.error(e);
    } finally {
      setImporting(false);
    }
  };

  const confirmImport = async () => {
    if (!preview) return;
    const list = preview.words;
    setPreview(null);
    try {
      await api.addDictionaryWords(list, "manual");
      toast(`${list.length} 个单词已添加`);
      load();
    } catch {
      toast(`添加 ${list.length} 个单词失败`, "error");
    }
  };

  const allSelected = visible.length > 0 && visible.every((w) => selected.has(w.id));

  return (
    <div className="dict page-scroll">
      <PageTitle
        right={
          <div className="btn-row">
            <Tooltip label="使用一列的 CSV 文件，每行一个单词。" side="bottom">
              <button className="btn btn-secondary" onClick={importCsv} disabled={importing}>
                <FileUp size={16} /> 导入 CSV
              </button>
            </Tooltip>
            <button className="btn btn-primary" onClick={() => setAddOpen(true)}>
              <Plus size={16} /> 新词
            </button>
          </div>
        }
      >
        词典
      </PageTitle>

      <p className="dict-note">
        <Info size={14} />
        Typeless 的词典保存在其云端，无法自动导入；可将词汇整理成单列 CSV（每行一个单词）后点击「导入 CSV」。
      </p>

      <div className="dict-toolbar">
        <Segmented id="dict-tabs" options={TABS} value={tab} onChange={setTab} />
        <div className="search">
          <Search size={15} />
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="搜索" spellCheck={false} />
          {query && (
            <button className="icon-btn xs" onClick={() => setQuery("")} aria-label="清除">
              <X size={13} />
            </button>
          )}
        </div>
      </div>

      <AnimatePresence initial={false}>
        {selected.size > 0 && (
          <motion.div
            className="selection-bar"
            initial={{ opacity: 0, height: 0, marginBottom: 0 }}
            animate={{ opacity: 1, height: 48, marginBottom: 12 }}
            exit={{ opacity: 0, height: 0, marginBottom: 0 }}
            transition={{ duration: 0.16 }}
          >
            <span className="sel-count">已选择 {selected.size} 个词</span>
            <button className="link-btn" onClick={() => setSelected(new Set(visible.map((w) => w.id)))} disabled={allSelected}>
              全选
            </button>
            <button className="link-btn" onClick={() => setSelected(new Set())}>
              清除选择
            </button>
            <div className="flex-spacer" />
            <button className="btn btn-danger btn-sm" onClick={removeSelected}>
              <Trash2 size={14} /> 删除 {selected.size} 个词
            </button>
          </motion.div>
        )}
      </AnimatePresence>

      {loaded && visible.length === 0 ? (
        <motion.div className="empty-state" initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }}>
          <div className="empty-icon">
            <BookOpen size={26} strokeWidth={1.6} />
          </div>
          <div className="empty-title">{query ? "没有结果" : "还没有词汇"}</div>
          <div className="empty-desc">{query ? "未找到与您的搜索匹配的单词。" : EMPTY_DESC[tab]}</div>
          {!query && (
            <button className="btn btn-primary" onClick={() => setAddOpen(true)}>
              <Plus size={16} /> 添加词汇
            </button>
          )}
        </motion.div>
      ) : (
        <div className="word-list">
          <AnimatePresence initial={false}>
            {visible.map((w) => (
              <motion.div
                key={w.id}
                layout="position"
                className={`word-row ${selected.has(w.id) ? "selected" : ""} ${selected.size > 0 ? "selecting" : ""}`}
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.14 }}
                onClick={() => selected.size > 0 && toggle(w.id)}
              >
                <div className="word-check">
                  <Checkbox checked={selected.has(w.id)} onChange={() => toggle(w.id)} />
                </div>
                <span className="word-text">{w.word}</span>
                <Tooltip label={w.source === "auto" ? "由 Hoshiro 自动添加" : "手动添加"}>
                  <span className="word-source">{w.source === "auto" ? <Sparkles size={14} /> : <PenLine size={14} />}</span>
                </Tooltip>
                <div className="word-actions" onClick={(e) => e.stopPropagation()}>
                  <Tooltip label="编辑">
                    <button className="icon-btn sm" onClick={() => setEditing(w)} aria-label="编辑">
                      <Pencil size={15} />
                    </button>
                  </Tooltip>
                  <Tooltip label="删除">
                    <button className="icon-btn sm" onClick={() => removeOne(w)} aria-label="删除">
                      <Trash2 size={15} />
                    </button>
                  </Tooltip>
                </div>
              </motion.div>
            ))}
          </AnimatePresence>
        </div>
      )}

      <WordModal
        open={addOpen}
        title="添加到词典"
        placeholder="添加新词"
        submitLabel="添加词汇"
        onClose={() => setAddOpen(false)}
        onSubmit={async (word) => {
          await api.addDictionaryWords([word], "manual");
          toast(`'${word}' 添加成功`);
          setAddOpen(false);
          load();
        }}
      />
      <WordModal
        open={!!editing}
        title="编辑词汇"
        placeholder="编辑词汇"
        submitLabel="保存"
        initial={editing?.word ?? ""}
        onClose={() => setEditing(null)}
        onSubmit={async (word) => {
          if (!editing) return;
          if (word !== editing.word) await api.updateDictionaryWord(editing.id, word);
          toast(`已更新 '${word}'`);
          setEditing(null);
          load();
        }}
      />

      <Modal open={!!preview} onClose={() => setPreview(null)} width={440}>
        <ModalHeader title="导入 CSV" onClose={() => setPreview(null)} />
        <div className="modal-body">
          <p className="import-count">
            {preview?.words.length} 个有效单词在 {preview?.fileName} 中找到
          </p>
          <div className="chip-cloud">
            {preview?.words.slice(0, 60).map((w, i) => (
              <span className="chip" key={i}>
                {w}
              </span>
            ))}
            {preview && preview.words.length > 60 && <span className="chip muted">… 另有 {preview.words.length - 60} 个</span>}
          </div>
        </div>
        <div className="modal-footer">
          <button className="btn btn-secondary" onClick={() => setPreview(null)}>
            取消
          </button>
          <button className="btn btn-primary" onClick={confirmImport}>
            添加单词
          </button>
        </div>
      </Modal>
    </div>
  );
}

function WordModal({
  open,
  title,
  placeholder,
  submitLabel,
  initial = "",
  onClose,
  onSubmit,
}: {
  open: boolean;
  title: string;
  placeholder: string;
  submitLabel: string;
  initial?: string;
  onClose: () => void;
  onSubmit: (word: string) => Promise<void>;
}) {
  const [value, setValue] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setValue(initial);
      setError(null);
      setSaving(false);
      window.setTimeout(() => inputRef.current?.select(), 60);
    }
  }, [open, initial]);

  const submit = async () => {
    const v = value.trim();
    if (!v) return setError("请输入一个词");
    if (v.length > MAX_LEN) return setError("名称超过了长度限制。");
    setSaving(true);
    try {
      await onSubmit(v);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal open={open} onClose={onClose} width={420}>
      <ModalHeader title={title} onClose={onClose} />
      <div className="modal-body">
        <input
          ref={inputRef}
          className={`input ${error ? "invalid" : ""}`}
          value={value}
          placeholder={placeholder}
          spellCheck={false}
          onChange={(e) => {
            setValue(e.target.value);
            setError(null);
          }}
          onKeyDown={(e) => e.key === "Enter" && !e.nativeEvent.isComposing && submit()}
        />
        {error && <div className="field-error">{error}</div>}
      </div>
      <div className="modal-footer">
        <button className="btn btn-secondary" onClick={onClose}>
          取消
        </button>
        <button className="btn btn-primary" onClick={submit} disabled={saving}>
          {submitLabel}
        </button>
      </div>
    </Modal>
  );
}
