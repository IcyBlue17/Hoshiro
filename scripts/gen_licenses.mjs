#!/usr/bin/env node
// Generates src/hub/about/licenses.json (third-party components shown in 关于 › 第三方开源组件).
// Rust: normal (non-dev, non-build) dependency graph of `hoshiro` for the host target.
// JS: runtime `dependencies` of package.json plus transitive production deps from node_modules.
// Usage: node scripts/gen_licenses.mjs   (needs cargo + installed node_modules)
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, "src/hub/about/licenses.json");

const cleanUrl = (u) =>
  typeof u === "string" && u
    ? u.replace(/^git\+/, "").replace(/\.git$/, "").replace(/^git:\/\//, "https://").replace(/^ssh:\/\/git@/, "https://")
    : undefined;

/* ───── Rust ───── */
function rust() {
  const manifest = join(root, "src-tauri/Cargo.toml");
  const host = /host: (\S+)/.exec(execFileSync("rustc", ["-vV"], { encoding: "utf8" }))?.[1];
  const args = ["metadata", "--format-version", "1", "--manifest-path", manifest];
  if (host) args.push("--filter-platform", host);
  const meta = JSON.parse(execFileSync("cargo", args, { encoding: "utf8", maxBuffer: 512 * 1024 * 1024 }));
  const pkgs = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const rootPkg = meta.packages.find((p) => p.name === "hoshiro" && p.manifest_path === manifest) ?? meta.packages.find((p) => p.name === "hoshiro");
  const seen = new Set([rootPkg.id]);
  const stack = [rootPkg.id];
  while (stack.length) {
    for (const d of nodes.get(stack.pop())?.deps ?? []) {
      // dep_kinds: kind null = normal, "dev", "build". Keep only edges with a normal kind.
      if (!d.dep_kinds.some((k) => k.kind === null)) continue;
      if (!seen.has(d.pkg)) {
        seen.add(d.pkg);
        stack.push(d.pkg);
      }
    }
  }
  seen.delete(rootPkg.id);
  return [...seen].map((id) => {
    const p = pkgs.get(id);
    return {
      name: p.name,
      version: p.version,
      license: p.license ?? (p.license_file ? "See license file" : "Unknown"),
      url: cleanUrl(p.repository) ?? cleanUrl(p.homepage) ?? `https://crates.io/crates/${p.name}`,
    };
  });
}

/* ───── JS ───── */
function js() {
  const readPkg = (dir) => {
    const f = join(dir, "package.json");
    return existsSync(f) ? JSON.parse(readFileSync(f, "utf8")) : null;
  };
  // Resolve like node: walk up node_modules from the requiring package dir.
  const find = (name, from) => {
    for (let dir = from; ; dir = dirname(dir)) {
      const cand = join(dir, "node_modules", name);
      if (existsSync(join(cand, "package.json"))) return cand;
      if (dir === root || dirname(dir) === dir) return null;
    }
  };
  const top = readPkg(root);
  const result = new Map();
  const visit = (name, from) => {
    const dir = find(name, from);
    if (!dir) return console.warn(`warn: ${name} not found in node_modules (optional?)`);
    const p = readPkg(dir);
    const key = `${p.name}@${p.version}`;
    if (result.has(key)) return;
    const lic = p.license ?? (Array.isArray(p.licenses) ? p.licenses.map((l) => l.type).join(" OR ") : undefined);
    const repo = typeof p.repository === "string" ? p.repository : p.repository?.url;
    const gh = /^[\w.-]+\/[\w.-]+$/.test(repo ?? "") ? `https://github.com/${repo}` : repo;
    result.set(key, {
      name: p.name,
      version: p.version,
      license: typeof lic === "object" ? lic.type : (lic ?? "Unknown"),
      url: cleanUrl(gh) ?? cleanUrl(p.homepage) ?? `https://www.npmjs.com/package/${p.name}`,
    });
    for (const dep of Object.keys(p.dependencies ?? {})) visit(dep, dir);
  };
  for (const dep of Object.keys(top.dependencies ?? {})) visit(dep, root);
  return [...result.values()];
}

const byName = (a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version, undefined, { numeric: true });
const data = { rust: rust().sort(byName), js: js().sort(byName) };
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, JSON.stringify(data, null, 1) + "\n");
console.log(`licenses.json: ${data.rust.length} Rust crates, ${data.js.length} JS packages`);
