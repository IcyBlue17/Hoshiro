import { Fragment, type ReactNode } from "react";
import { api } from "../lib/ipc";

// Tiny markdown subset for answers: paragraphs, headings (as bold lines), bullet/numbered lists,
// block quotes, fenced code blocks, `inline code`, **bold**, *italic*, [links](https://…).

const INLINE = /(`[^`\n]+`)|(\*\*[^*\n]+\*\*)|(__[^_\n]+__)|(\*[^*\s][^*\n]*\*)|(\[[^\]\n]+\]\(https?:\/\/[^)\s]+\))/g;

export function renderInline(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  let last = 0;
  let key = 0;
  for (const m of text.matchAll(INLINE)) {
    const i = m.index ?? 0;
    if (i > last) out.push(text.slice(last, i));
    const s = m[0];
    if (m[1]) out.push(<code key={key++}>{s.slice(1, -1)}</code>);
    else if (m[2] || m[3]) out.push(<strong key={key++}>{s.slice(2, -2)}</strong>);
    else if (m[4]) out.push(<em key={key++}>{s.slice(1, -1)}</em>);
    else if (m[5]) {
      const mm = /^\[([^\]]+)\]\(([^)]+)\)$/.exec(s)!;
      out.push(
        <a key={key++} href={mm[2]} onClick={(e) => { e.preventDefault(); void api.openUrl(mm[2]); }}>
          {mm[1]}
        </a>,
      );
    }
    last = i + s.length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

const BULLET = /^\s*[-*•+]\s+(.*)$/;
const NUMBERED = /^\s*(\d+)[.)]\s+(.*)$/;
const HEADING = /^\s{0,3}#{1,6}\s+(.*)$/;
const FENCE = /^\s*```/;

export function Markdown({ text }: { text: string }) {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const blocks: ReactNode[] = [];
  let i = 0;
  let key = 0;
  const special = (l: string) => BULLET.test(l) || NUMBERED.test(l) || HEADING.test(l) || FENCE.test(l) || /^\s*>/.test(l);

  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) { i++; continue; }

    if (FENCE.test(line)) {
      const code: string[] = [];
      i++;
      while (i < lines.length && !FENCE.test(lines[i])) code.push(lines[i++]);
      i++; // closing fence
      blocks.push(<pre key={key++}><code>{code.join("\n")}</code></pre>);
      continue;
    }
    const h = HEADING.exec(line);
    if (h) { blocks.push(<p key={key++} className="md-h">{renderInline(h[1])}</p>); i++; continue; }

    if (BULLET.test(line) || NUMBERED.test(line)) {
      const ordered = NUMBERED.test(line) && !BULLET.test(line);
      const start = ordered ? parseInt(NUMBERED.exec(line)![1], 10) : 1;
      const items: ReactNode[][] = [];
      while (i < lines.length) {
        const l = lines[i];
        const b = ordered ? NUMBERED.exec(l) : BULLET.exec(l);
        if (b) { items.push(renderInline(b[b.length - 1])); i++; continue; }
        // indented continuation of the previous item
        if (l.trim() && /^\s{2,}\S/.test(l) && items.length && !special(l.trimStart())) {
          items[items.length - 1].push(<br key={`br${i}`} />, ...renderInline(l.trim()));
          i++;
          continue;
        }
        break;
      }
      const lis = items.map((it, n) => <li key={n}>{it}</li>);
      blocks.push(ordered ? <ol key={key++} start={start}>{lis}</ol> : <ul key={key++}>{lis}</ul>);
      continue;
    }

    if (/^\s*>/.test(line)) {
      const q: string[] = [];
      while (i < lines.length && /^\s*>/.test(lines[i])) q.push(lines[i++].replace(/^\s*>\s?/, ""));
      blocks.push(<blockquote key={key++}>{renderInline(q.join(" "))}</blockquote>);
      continue;
    }

    const para: string[] = [];
    while (i < lines.length && lines[i].trim() && !special(lines[i])) para.push(lines[i++]);
    if (!para.length) { para.push(lines[i++]); }
    blocks.push(
      <p key={key++}>
        {para.map((l, n) => (
          <Fragment key={n}>
            {n > 0 && <br />}
            {renderInline(l)}
          </Fragment>
        ))}
      </p>,
    );
  }
  return <div className="md">{blocks}</div>;
}
