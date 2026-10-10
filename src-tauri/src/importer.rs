//! Typeless migration: settings from `app-settings.json`, history from `typeless.db`.

use crate::db::{normalize_ts, Db, HistoryRow};
use crate::stats::count_words;
use crate::types::{
    Mode, Settings, ShortcutBindings, TypelessDetection, TypelessImportOptions, TypelessImportResult,
};
use anyhow::Context;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const TYPELESS_BUNDLE_ID: &str = "now.typeless.desktop";

pub fn typeless_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("Typeless"))
}

pub fn typeless_installed() -> bool {
    if Path::new("/Applications/Typeless.app").exists() {
        return true;
    }
    dirs::home_dir().is_some_and(|h| h.join("Applications/Typeless.app").exists())
}

// ───────────────────────── settings ─────────────────────────

const KEY_NAMES: &[&str] = &[
    "Fn", "Help", "Esc", "Space", "Tab", "Enter", "Backspace", "LeftShift", "RightShift", "Shift", "LeftCtrl",
    "RightCtrl", "Ctrl", "LeftOption", "RightOption", "Option", "LeftCmd", "RightCmd", "Cmd", "Up", "Down", "Left",
    "Right", "Mouse3", "Mouse4", "Mouse5",
];

/// Map a Typeless key name to ours (best effort). None = unknown key.
pub fn normalize_key_name(name: &str) -> Option<String> {
    let n = name.trim();
    if n.is_empty() {
        return None;
    }
    if let Some(k) = KEY_NAMES.iter().find(|k| k.eq_ignore_ascii_case(n)) {
        return Some((*k).to_string());
    }
    let lower = n.to_ascii_lowercase();
    let mapped = match lower.as_str() {
        "control" => "Ctrl",
        "leftcontrol" | "controlleft" | "lctrl" => "LeftCtrl",
        "rightcontrol" | "controlright" | "rctrl" => "RightCtrl",
        "alt" | "opt" => "Option",
        "leftalt" | "altleft" | "leftopt" | "lalt" | "loption" => "LeftOption",
        "rightalt" | "altright" | "rightopt" | "ralt" | "roption" => "RightOption",
        "command" | "meta" | "super" | "win" | "windows" => "Cmd",
        "leftcommand" | "leftmeta" | "metaleft" | "lcmd" | "leftsuper" => "LeftCmd",
        "rightcommand" | "rightmeta" | "metaright" | "rcmd" | "rightsuper" => "RightCmd",
        "lshift" | "shiftleft" => "LeftShift",
        "rshift" | "shiftright" => "RightShift",
        "return" => "Enter",
        "escape" => "Esc",
        "arrowup" => "Up",
        "arrowdown" => "Down",
        "arrowleft" => "Left",
        "arrowright" => "Right",
        "globe" | "function" => "Fn",
        _ => "",
    };
    if !mapped.is_empty() {
        return Some(mapped.to_string());
    }
    // Letters, digits, F-keys
    let stripped = lower.strip_prefix("key").or_else(|| lower.strip_prefix("digit")).unwrap_or(&lower);
    if stripped.chars().count() == 1 {
        let c = stripped.chars().next()?;
        if c.is_ascii_alphanumeric() {
            return Some(c.to_ascii_uppercase().to_string());
        }
    }
    if let Some(num) = lower.strip_prefix('f').and_then(|r| r.parse::<u8>().ok()) {
        if (1..=20).contains(&num) {
            return Some(format!("F{num}"));
        }
    }
    None
}

pub fn normalize_shortcut(s: &str) -> Option<String> {
    let parts: Option<Vec<String>> = s.split('+').map(normalize_key_name).collect();
    let parts = parts?;
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("+"))
    }
}

fn map_bindings(v: Option<&Value>, label: &str, messages: &mut Vec<String>) -> Option<Vec<String>> {
    let arr = v?.as_array()?;
    let mut out = Vec::new();
    for item in arr.iter().filter_map(Value::as_str) {
        match normalize_shortcut(item) {
            Some(s) if !out.contains(&s) => out.push(s),
            Some(_) => {}
            None => messages.push(format!("{label}快捷键 “{item}” 无法识别，已跳过")),
        }
    }
    Some(out)
}

pub fn retention_from_seconds(v: Option<&Value>) -> &'static str {
    let secs = match v {
        None | Some(Value::Null) => return "forever",
        Some(x) => x.as_f64().unwrap_or(-1.0),
    };
    if secs < 0.0 {
        "forever"
    } else if secs == 0.0 {
        "never"
    } else if secs <= 86_400.0 {
        "24h"
    } else if secs <= 604_800.0 {
        "1w"
    } else if secs <= 2_592_000.0 {
        "1m"
    } else if secs <= 31_536_000.0 {
        "1y"
    } else {
        "forever"
    }
}

/// Parse only the shortcut bindings (for detection).
pub fn shortcuts_from_json(v: &Value) -> Option<ShortcutBindings> {
    let b = v.get("featureShortcutBindings")?;
    let mut msgs = Vec::new();
    let def = ShortcutBindings::default();
    let dictation = map_bindings(b.get("dictationMode"), "口述", &mut msgs).filter(|x| !x.is_empty());
    Some(ShortcutBindings {
        trigger: def.trigger,
        dictation: dictation.unwrap_or(def.dictation),
        ask_anything: map_bindings(b.get("askAnythingMode"), "随便问", &mut msgs).unwrap_or(def.ask_anything),
        translation: map_bindings(b.get("translationMode"), "翻译", &mut msgs).unwrap_or(def.translation),
        paste_last_transcript: map_bindings(b.get("pasteLastTranscript"), "粘贴上一次转录", &mut msgs)
            .unwrap_or(def.paste_last_transcript),
    })
}

/// Overlay Typeless' `app-settings.json` onto `base`. Only fields present in the file are touched
/// (API keys and provider settings are never affected).
pub fn map_settings(v: &Value, base: &Settings) -> (Settings, Vec<String>) {
    let mut s = base.clone();
    let mut messages = Vec::new();

    if let Some(b) = v.get("featureShortcutBindings") {
        if let Some(d) = map_bindings(b.get("dictationMode"), "口述", &mut messages).filter(|x| !x.is_empty()) {
            s.shortcuts.dictation = d;
        }
        if let Some(x) = map_bindings(b.get("askAnythingMode"), "随便问", &mut messages) {
            s.shortcuts.ask_anything = x;
        }
        if let Some(x) = map_bindings(b.get("translationMode"), "翻译", &mut messages) {
            s.shortcuts.translation = x;
        }
        if let Some(x) = map_bindings(b.get("pasteLastTranscript"), "粘贴上一次转录", &mut messages) {
            s.shortcuts.paste_last_transcript = x;
        }
    }

    let str_list = |key: &str| -> Option<Vec<String>> {
        let l: Vec<String> = v
            .get(key)?
            .as_array()?
            .iter()
            .filter_map(Value::as_str)
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect();
        (!l.is_empty()).then_some(l)
    };
    if let Some(l) = str_list("selectedLanguages") {
        s.selected_languages = l;
    }
    if let Some(l) = str_list("translationModeTargetLanguageCodes") {
        s.translation_targets = l.into_iter().take(3).collect();
    }
    let flag = |key: &str| v.get(key).and_then(Value::as_bool);
    if let Some(b) = flag("autoSelectLanguages") {
        s.auto_select_languages = b;
    }
    if let Some(b) = flag("launchAtSystemStartup") {
        s.launch_at_login = b;
    }
    if let Some(b) = flag("showFlowBarOnDesktop") {
        s.show_bar_on_desktop = b;
    }
    if let Some(b) = flag("enableInteractionSoundEffects") {
        s.interaction_sounds = b;
    }
    if let Some(b) = flag("enableShowAppInDock") {
        s.show_in_dock = b;
    }
    if let Some(b) = flag("enabledMuteBackgroundAudio") {
        s.mute_while_dictating = b;
    }
    if let Some(a) = v.get("appearancePreference").and_then(Value::as_str) {
        if ["system", "light", "dark"].contains(&a) {
            s.appearance = a.to_string();
        }
    }
    if v.get("historyDurationSeconds").is_some() {
        s.history_retention = retention_from_seconds(v.get("historyDurationSeconds")).to_string();
    }
    (s, messages)
}

// ───────────────────────── history ─────────────────────────

pub fn map_mode(m: Option<&str>) -> Mode {
    match m.unwrap_or("voice_transcript") {
        "voice_translation" => Mode::Translation,
        "voice_command" | "ask" | "voice_ask" => Mode::Ask,
        _ => Mode::Dictation,
    }
}

pub fn map_status(status: Option<&str>, has_text: bool) -> &'static str {
    match status.map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("completed") | Some("complete") | Some("success") | Some("done") => {
            if has_text {
                "completed"
            } else {
                "empty"
            }
        }
        Some("dismissed") | Some("cancelled") | Some("canceled") | Some("aborted") => "dismissed",
        Some("failed") | Some("error") => "error",
        Some("empty") | Some("no_speech") | Some("silent") => "empty",
        _ => {
            if has_text {
                "completed"
            } else {
                "empty"
            }
        }
    }
}

struct LegacyInfo {
    app_name: Option<String>,
    bundle_id: Option<String>,
    window_title: Option<String>,
}

fn build_row(
    id: String,
    status: Option<String>,
    mode: Option<String>,
    refined_col: Option<String>,
    duration: Option<f64>,
    created_at: Option<String>,
    mode_meta: Option<String>,
    legacy: Option<&LegacyInfo>,
) -> HistoryRow {
    let meta: Option<Value> = mode_meta.as_deref().and_then(|m| serde_json::from_str(m).ok());
    let ai = meta.as_ref().and_then(|m| m.get("ai_result"));
    let from_meta = ai.and_then(|a| a.get("refined_text")).and_then(Value::as_str).map(String::from);
    let refined = refined_col.filter(|t| !t.trim().is_empty()).or(from_meta.filter(|t| !t.trim().is_empty()));
    let prompt = ai
        .and_then(|a| a.get("user_prompt"))
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
        .map(String::from);

    let mode = map_mode(mode.as_deref());
    let status = map_status(status.as_deref(), refined.is_some());
    let mut row = HistoryRow::new(id, mode, status);
    row.created_at = created_at.as_deref().and_then(normalize_ts).unwrap_or_else(crate::db::now_ts);
    row.duration_secs = duration.filter(|d| d.is_finite() && *d >= 0.0).unwrap_or(0.0);
    row.words = refined.as_deref().map(count_words).unwrap_or(0);
    row.refined_text = refined;
    if mode == Mode::Ask {
        row.raw_text = prompt;
    }
    if let Some(l) = legacy {
        row.app_name = l.app_name.clone();
        row.bundle_id = l.bundle_id.clone();
        row.window_title = l.window_title.clone();
    }
    row.source = "typeless".into();
    row
}

/// Copy the Typeless database (+ WAL/SHM) to a temp dir and open that copy read-only.
fn open_copy(src_dir: &Path) -> anyhow::Result<(Connection, PathBuf)> {
    let db = src_dir.join("typeless.db");
    if !db.is_file() {
        anyhow::bail!("未找到 Typeless 的历史数据库");
    }
    let tmp = std::env::temp_dir().join(format!("hoshiro-typeless-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp)?;
    let copy = |name: &str| -> anyhow::Result<()> {
        let from = src_dir.join(name);
        if from.is_file() {
            std::fs::copy(&from, tmp.join(name)).with_context(|| format!("复制 {name} 失败"))?;
        }
        Ok(())
    };
    let res = (|| {
        copy("typeless.db")?;
        let _ = copy("typeless.db-wal");
        let _ = copy("typeless.db-shm");
        let conn = Connection::open_with_flags(
            tmp.join("typeless.db"),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        Ok::<_, anyhow::Error>(conn)
    })();
    match res {
        Ok(c) => Ok((c, tmp)),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            Err(e)
        }
    }
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1", [name], |_| Ok(())).is_ok()
}

pub fn count_typeless_history(src_dir: &Path) -> anyhow::Result<u32> {
    let (conn, tmp) = open_copy(src_dir)?;
    let n = (|| -> anyhow::Result<u32> {
        let mut total = 0u32;
        if table_exists(&conn, "history_v2") {
            total += conn.query_row("SELECT COUNT(*) FROM history_v2", [], |r| r.get::<_, i64>(0))?.max(0) as u32;
        }
        Ok(total)
    })();
    drop(conn);
    let _ = std::fs::remove_dir_all(&tmp);
    n
}

pub fn read_history_rows(src_dir: &Path) -> anyhow::Result<Vec<HistoryRow>> {
    let (conn, tmp) = open_copy(src_dir)?;
    let res = read_rows_from(&conn);
    drop(conn);
    let _ = std::fs::remove_dir_all(&tmp);
    res
}

fn read_rows_from(conn: &Connection) -> anyhow::Result<Vec<HistoryRow>> {
    let mut legacy: std::collections::HashMap<String, LegacyInfo> = std::collections::HashMap::new();
    let mut legacy_only: Vec<HistoryRow> = Vec::new();
    let has_v2 = table_exists(conn, "history_v2");
    let mut v2_ids = std::collections::HashSet::new();

    let mut rows = Vec::new();
    if has_v2 {
        let mut stmt = conn.prepare(
            "SELECT id, status, mode, refined_text, duration, created_at, CAST(mode_meta AS TEXT) FROM history_v2",
        )?;
        let it = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<f64>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })?;
        for x in it {
            let (id, status, mode, refined, dur, created, meta) = x?;
            v2_ids.insert(id.clone());
            rows.push((id, status, mode, refined, dur, created, meta));
        }
    }

    if table_exists(conn, "history") {
        let q = conn.prepare(
            "SELECT id, status, mode, refined_text, duration, created_at, CAST(mode_meta AS TEXT), \
             focused_app_name, focused_app_bundle_id, focused_app_window_title FROM history",
        );
        if let Ok(mut stmt) = q {
            let it = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<f64>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    LegacyInfo {
                        app_name: r.get(7)?,
                        bundle_id: r.get(8)?,
                        window_title: r.get(9)?,
                    },
                ))
            })?;
            for x in it.flatten() {
                let (id, status, mode, refined, dur, created, meta, info) = x;
                if v2_ids.contains(&id) {
                    legacy.insert(id, info);
                } else {
                    legacy_only.push(build_row(id, status, mode, refined, dur, created, meta, Some(&info)));
                }
            }
        }
    }

    let mut out: Vec<HistoryRow> = rows
        .into_iter()
        .map(|(id, status, mode, refined, dur, created, meta)| {
            let info = legacy.get(&id);
            build_row(id, status, mode, refined, dur, created, meta, info)
        })
        .collect();
    out.extend(legacy_only);
    Ok(out)
}

// ───────────────────────── public operations ─────────────────────────

pub fn detect() -> TypelessDetection {
    let dir = typeless_dir();
    let installed = typeless_installed();
    let running = !crate::platform::permissions::running_pids(TYPELESS_BUNDLE_ID).is_empty();
    let mut d = TypelessDetection { installed, running, ..Default::default() };
    let Some(dir) = dir else { return d };
    let settings_file = dir.join("app-settings.json");
    let has_db = dir.join("typeless.db").is_file();
    d.data_found = settings_file.is_file() || has_db;
    if let Some(v) = std::fs::read_to_string(&settings_file).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        d.shortcuts = shortcuts_from_json(&v);
    }
    if has_db {
        match count_typeless_history(&dir) {
            Ok(n) => d.history_count = n,
            Err(e) => log::warn!("cannot count Typeless history: {e:#}"),
        }
    }
    d
}

pub fn read_settings_json(dir: &Path) -> anyhow::Result<Value> {
    let path = dir.join("app-settings.json");
    let text = std::fs::read_to_string(&path).context("无法读取 Typeless 设置文件")?;
    serde_json::from_str(&text).context("Typeless 设置文件格式无法识别")
}

pub fn import_history(dir: &Path, db: &Db) -> anyhow::Result<(u32, u32)> {
    let rows = read_history_rows(dir)?;
    db.import_rows(&rows)
}

pub fn fill_messages(result: &mut TypelessImportResult, opts: &TypelessImportOptions) {
    if opts.history {
        result.messages.push("录音文件不会被迁移，导入的历史记录仅包含文字。".into());
    }
    result.messages.push("Typeless 的词典保存在云端，无法直接导入；可在 Typeless 中导出 CSV 后在词典页导入。".into());
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        json!({
            "featureShortcutBindings": {
                "dictationMode": ["Fn", "Help", "RightOption"],
                "askAnythingMode": ["Fn+Space"],
                "translationMode": ["Fn+LeftShift"],
                "pasteLastTranscript": ["Ctrl+Cmd+V"]
            },
            "selectedLanguages": ["zh-CN"],
            "autoSelectLanguages": false,
            "launchAtSystemStartup": true,
            "showFlowBarOnDesktop": false,
            "enableInteractionSoundEffects": true,
            "enableShowAppInDock": true,
            "historyDurationSeconds": 86400,
            "enabledMuteBackgroundAudio": true,
            "translationModeTargetLanguageCodes": ["en-US"],
            "appearancePreference": "system"
        })
    }

    #[test]
    fn maps_fixture_settings() {
        let mut base = Settings::default();
        base.asr.mimo.api_key = "keep-me".into();
        base.show_in_dock = false;
        let (s, msgs) = map_settings(&fixture(), &base);
        assert!(msgs.is_empty());
        assert_eq!(s.shortcuts.dictation, vec!["Fn", "Help", "RightOption"]);
        assert_eq!(s.shortcuts.ask_anything, vec!["Fn+Space"]);
        assert_eq!(s.shortcuts.translation, vec!["Fn+LeftShift"]);
        assert_eq!(s.shortcuts.paste_last_transcript, vec!["Ctrl+Cmd+V"]);
        assert_eq!(s.selected_languages, vec!["zh-CN"]);
        assert!(!s.auto_select_languages);
        assert!(s.launch_at_login);
        assert!(!s.show_bar_on_desktop);
        assert!(s.interaction_sounds);
        assert!(s.show_in_dock);
        assert!(s.mute_while_dictating);
        assert_eq!(s.history_retention, "24h");
        assert_eq!(s.translation_targets, vec!["en-US"]);
        assert_eq!(s.appearance, "system");
        assert_eq!(s.asr.mimo.api_key, "keep-me");
    }

    #[test]
    fn missing_fields_leave_base_untouched() {
        let base = Settings { show_in_dock: false, ..Settings::default() };
        let (s, _) = map_settings(&json!({"appearancePreference": "dark"}), &base);
        assert_eq!(s.appearance, "dark");
        assert!(!s.show_in_dock);
        assert_eq!(s.history_retention, base.history_retention);
    }

    #[test]
    fn retention_mapping() {
        assert_eq!(retention_from_seconds(Some(&json!(86400))), "24h");
        assert_eq!(retention_from_seconds(Some(&json!(604800))), "1w");
        assert_eq!(retention_from_seconds(Some(&json!(2592000))), "1m");
        assert_eq!(retention_from_seconds(Some(&json!(31536000))), "1y");
        assert_eq!(retention_from_seconds(Some(&json!(-1))), "forever");
        assert_eq!(retention_from_seconds(Some(&Value::Null)), "forever");
        assert_eq!(retention_from_seconds(None), "forever");
        assert_eq!(retention_from_seconds(Some(&json!(0))), "never");
    }

    #[test]
    fn key_name_mapping() {
        assert_eq!(normalize_shortcut("fn+space").as_deref(), Some("Fn+Space"));
        assert_eq!(normalize_shortcut("Control+Command+v").as_deref(), Some("Ctrl+Cmd+V"));
        assert_eq!(normalize_shortcut("RightAlt").as_deref(), Some("RightOption"));
        assert_eq!(normalize_shortcut("KeyA").as_deref(), Some("A"));
        assert_eq!(normalize_shortcut("f13").as_deref(), Some("F13"));
        assert_eq!(normalize_shortcut("Fn+Bogus"), None);
    }

    #[test]
    fn unknown_binding_is_reported_and_skipped() {
        let v = json!({"featureShortcutBindings": {"dictationMode": ["Fn", "Wat"], "askAnythingMode": []}});
        let (s, msgs) = map_settings(&v, &Settings::default());
        assert_eq!(s.shortcuts.dictation, vec!["Fn"]);
        assert!(s.shortcuts.ask_anything.is_empty());
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn mode_and_status_mapping() {
        assert_eq!(map_mode(Some("voice_transcript")), Mode::Dictation);
        assert_eq!(map_mode(Some("voice_translation")), Mode::Translation);
        assert_eq!(map_mode(Some("voice_command")), Mode::Ask);
        assert_eq!(map_mode(None), Mode::Dictation);
        assert_eq!(map_status(Some("completed"), true), "completed");
        assert_eq!(map_status(Some("completed"), false), "empty");
        assert_eq!(map_status(Some("dismissed"), false), "dismissed");
        assert_eq!(map_status(Some("failed"), false), "error");
    }

    #[test]
    fn imports_history_from_fixture_db() {
        let src = std::env::temp_dir().join(format!("hoshiro-imp-src-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&src).unwrap();
        {
            let c = Connection::open(src.join("typeless.db")).unwrap();
            c.execute_batch(
                "CREATE TABLE history_v2 (id text PRIMARY KEY, status text, mode text, refined_text text, duration real,
                    created_at text, mode_meta blob);
                 CREATE TABLE history (id text PRIMARY KEY, status text, mode text, refined_text text, duration real,
                    created_at text, mode_meta blob, focused_app_name text, focused_app_bundle_id text,
                    focused_app_window_title text);
                 INSERT INTO history_v2 VALUES ('a','completed','voice_transcript','你好 world',2.5,'2026-10-09T18:57:31.104Z',NULL);
                 INSERT INTO history_v2 VALUES ('b','completed','voice_command',NULL,1.0,'2026-10-09T19:00:00.000Z',
                    CAST('{\"ai_result\":{\"user_prompt\":\"翻译\",\"refined_text\":\"Hello\"}}' AS BLOB));
                 INSERT INTO history_v2 VALUES ('c','dismissed','voice_transcript',NULL,0.5,'2026-10-09T16:58:51.611Z',NULL);
                 INSERT INTO history VALUES ('a','completed','voice_transcript','x',1.0,'2026-10-09T18:57:31.104Z',NULL,'Notes','com.apple.Notes','Doc');
                 INSERT INTO history VALUES ('old','completed','voice_translation','Hi',1.0,'2026-01-01T00:00:00.000Z',NULL,'Mail','com.apple.mail',NULL);",
            )
            .unwrap();
        }
        assert_eq!(count_typeless_history(&src).unwrap(), 3);
        let rows = read_history_rows(&src).unwrap();
        assert_eq!(rows.len(), 4);
        let a = rows.iter().find(|r| r.id == "a").unwrap();
        assert_eq!(a.refined_text.as_deref(), Some("你好 world"));
        assert_eq!(a.app_name.as_deref(), Some("Notes"));
        assert_eq!(a.words, 3);
        assert_eq!(a.source, "typeless");
        assert!(a.audio_path.is_none());
        let b = rows.iter().find(|r| r.id == "b").unwrap();
        assert_eq!(b.mode, Mode::Ask);
        assert_eq!(b.refined_text.as_deref(), Some("Hello"));
        assert_eq!(b.raw_text.as_deref(), Some("翻译"));
        assert_eq!(rows.iter().find(|r| r.id == "c").unwrap().status, "dismissed");
        assert_eq!(rows.iter().find(|r| r.id == "old").unwrap().mode, Mode::Translation);

        let dst = std::env::temp_dir().join(format!("hoshiro-imp-dst-{}", uuid::Uuid::new_v4()));
        let db = Db::open_in_memory(dst.clone()).unwrap();
        assert_eq!(import_history(&src, &db).unwrap(), (4, 0));
        assert_eq!(import_history(&src, &db).unwrap(), (0, 4));
        let _ = std::fs::remove_dir_all(src);
        let _ = std::fs::remove_dir_all(dst);
    }
}
