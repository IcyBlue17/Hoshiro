import { useMemo, useRef, useState } from "react";
import { ChevronRight, ExternalLink, FileText, Package, Search } from "lucide-react";
import { api } from "../../lib/ipc";
import { useApp } from "../store";
import { useFeedback } from "../feedback";
import { Modal, ModalHeader } from "../ui";
import { Group, Row } from "../settings/shared";
import logo from "../assets/logo.png";
import licenseText from "../../../LICENSE?raw";
import data from "./licenses.json";

interface Component {
  name: string;
  version: string;
  license: string;
  url: string;
}
const RUST = data.rust as Component[];
const JS = data.js as Component[];

export const REPO_URL = "https://github.com/IcyBlue17/hoshiro";
export const TAGLINE = "按住 Fn 说话，松开就是写好的字";

export function AboutSection() {
  const { version } = useApp();
  const { toast } = useFeedback();
  const [lic, setLic] = useState(false);
  const [deps, setDeps] = useState(false);
  const open = (url: string) => api.openUrl(url).catch((e) => toast(`无法打开链接：${String(e)}`, "error"));
  return (
    <>
      <div className="about-hero">
        <img src={logo} alt="" className="about-logo" draggable={false} />
        <div className="about-name">Hoshiro</div>
        <div className="about-version">版本 {version || "0.1.0"}</div>
        <div className="about-tagline">{TAGLINE}</div>
      </div>
      <Group>
        <Row title="GitHub" desc="IcyBlue17/hoshiro · 源码、反馈与发布">
          <button className="btn btn-secondary" onClick={() => open(REPO_URL)}>
            打开仓库 <ExternalLink size={13} />
          </button>
        </Row>
        <Row title="反馈问题" desc="遇到 bug 或有想法，欢迎开个 Issue。">
          <button className="btn btn-secondary" onClick={() => open(`${REPO_URL}/issues`)}>
            Issues <ExternalLink size={13} />
          </button>
        </Row>
      </Group>
      <Group>
        <button className="about-link" onClick={() => setLic(true)}>
          <FileText size={18} />
          <span className="about-link-text">
            <span className="srow-title">开源许可</span>
            <span className="srow-desc">Hoshiro 以此协议开放源码</span>
          </span>
          <span className="about-link-value">MIT License</span>
          <ChevronRight size={16} />
        </button>
        <button className="about-link" onClick={() => setDeps(true)}>
          <Package size={18} />
          <span className="about-link-text">
            <span className="srow-title">第三方开源组件</span>
            <span className="srow-desc">Rust {RUST.length} · JavaScript {JS.length}</span>
          </span>
          <span className="about-link-value">{RUST.length + JS.length} 个</span>
          <ChevronRight size={16} />
        </button>
      </Group>
      <Group>
        <Row title="隐私" desc="历史记录、词典和 API Key 仅保存在此设备上；音频与文字只会发送给您自行配置的语音识别与改写服务商。" />
      </Group>
      <p className="about-footer">Hoshiro 的诞生离不开这些开源项目 ❤︎</p>

      <Modal open={lic} onClose={() => setLic(false)} width={560}>
        <ModalHeader title="开源许可" subtitle="MIT License" onClose={() => setLic(false)} />
        <div className="modal-body">
          <div className="license-text">
            {licenseText.trim().split(/\n\s*\n/).map((para, i) => (
              <p key={i}>{para.replace(/\s*\n\s*/g, " ")}</p>
            ))}
          </div>
        </div>
        <div className="modal-footer" />
      </Modal>
      <ComponentsModal open={deps} onClose={() => setDeps(false)} onOpenUrl={open} />
    </>
  );
}

type Item = { kind: "head"; label: string; count: number } | { kind: "row"; c: Component; key: string };
const ROW_H = 46;
const HEAD_H = 36;
const VIEW_H = 380;

function ComponentsModal({ open, onClose, onOpenUrl }: { open: boolean; onClose: () => void; onOpenUrl: (u: string) => void }) {
  return (
    <Modal open={open} onClose={onClose} width={640}>
      <ComponentsBody onClose={onClose} onOpenUrl={onOpenUrl} />
    </Modal>
  );
}

// Mounted only while the modal is open, so search/scroll state resets each time.
function ComponentsBody({ onClose, onOpenUrl }: { onClose: () => void; onOpenUrl: (u: string) => void }) {
  const [q, setQ] = useState("");
  const [top, setTop] = useState(0);
  const scroller = useRef<HTMLDivElement>(null);

  const { items, offsets, total, shown } = useMemo(() => {
    const needle = q.trim().toLowerCase();
    const match = (c: Component) => !needle || c.name.toLowerCase().includes(needle) || c.license.toLowerCase().includes(needle);
    const items: Item[] = [];
    let shown = 0;
    for (const [label, list, eco] of [["Rust", RUST, "r"], ["JavaScript", JS, "j"]] as const) {
      const hit = list.filter(match);
      if (!hit.length) continue;
      shown += hit.length;
      items.push({ kind: "head", label, count: hit.length });
      for (const c of hit) items.push({ kind: "row", c, key: `${eco}:${c.name}@${c.version}` });
    }
    const offsets: number[] = [];
    let y = 0;
    for (const it of items) {
      offsets.push(y);
      y += it.kind === "head" ? HEAD_H : ROW_H;
    }
    return { items, offsets, total: y, shown };
  }, [q]);

  // binary search for the first visible item, then render a small window
  let lo = 0;
  let hi = items.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (offsets[mid] + (items[mid].kind === "head" ? HEAD_H : ROW_H) <= top) lo = mid + 1;
    else hi = mid;
  }
  const visible: number[] = [];
  for (let i = Math.max(0, lo - 4); i < items.length && offsets[i] < top + VIEW_H + 4 * ROW_H; i++) visible.push(i);

  return (
    <>
      <ModalHeader
        title="第三方开源组件"
        subtitle={
          <>
            共 {RUST.length + JS.length} 个运行时依赖。其中 MPL-2.0 为文件级弱 copyleft 许可，可正常随应用分发，仅供知悉。
          </>
        }
        onClose={onClose}
      />
      <div className="modal-body deps-body">
        <label className="deps-search">
          <Search size={15} />
          <input
            placeholder="搜索名称或许可证"
            value={q}
            spellCheck={false}
            onChange={(e) => {
              setQ(e.target.value);
              setTop(0);
              scroller.current?.scrollTo({ top: 0 });
            }}
          />
          <span className="muted">{shown}</span>
        </label>
        <div className="deps-scroll" ref={scroller} style={{ height: VIEW_H }} onScroll={(e) => setTop(e.currentTarget.scrollTop)}>
          <div style={{ height: total, position: "relative" }}>
            {visible.map((i) => {
              const it = items[i];
              if (it.kind === "head")
                return (
                  <div key={`h${it.label}`} className="deps-head" style={{ top: offsets[i], height: HEAD_H }}>
                    {it.label}
                    <span>{it.count}</span>
                  </div>
                );
              const { c } = it;
              return (
                <button key={it.key} className="deps-row" style={{ top: offsets[i], height: ROW_H }} onClick={() => onOpenUrl(c.url)} title={c.url}>
                  <span className="deps-name">{c.name}</span>
                  <span className="deps-ver">{c.version}</span>
                  <span className={`deps-badge ${/MPL/i.test(c.license) ? "mpl" : ""}`} title={c.license}>
                    {c.license}
                  </span>
                  <ExternalLink size={13} className="deps-go" />
                </button>
              );
            })}
          </div>
          {shown === 0 && <div className="deps-empty">没有匹配的组件</div>}
        </div>
      </div>
      <div className="modal-footer" />
    </>
  );
}
