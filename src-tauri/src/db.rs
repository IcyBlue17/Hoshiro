//! SQLite storage: history + dictionary. All access goes through one mutex-guarded connection;
//! call from `spawn_blocking` in async contexts.

use crate::stats::StatRow;
use crate::types::{DictionaryWord, HistoryItem, HistoryQuery, Mode};
use anyhow::Context;
use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: i64 = 1;

pub struct Db {
    conn: Mutex<Connection>,
    recordings_dir: PathBuf,
}

#[derive(Clone, Debug)]
pub struct HistoryRow {
    pub id: String,
    pub created_at: String,
    pub mode: Mode,
    pub status: String,
    pub raw_text: Option<String>,
    pub refined_text: Option<String>,
    pub selected_text: Option<String>,
    pub duration_secs: f64,
    pub app_name: Option<String>,
    pub bundle_id: Option<String>,
    pub window_title: Option<String>,
    pub error_message: Option<String>,
    /// File name inside the recordings directory.
    pub audio_path: Option<String>,
    pub source: String,
    pub words: u32,
}

impl HistoryRow {
    pub fn new(id: String, mode: Mode, status: &str) -> Self {
        Self {
            id,
            created_at: now_ts(),
            mode,
            status: status.to_string(),
            raw_text: None,
            refined_text: None,
            selected_text: None,
            duration_secs: 0.0,
            app_name: None,
            bundle_id: None,
            window_title: None,
            error_message: None,
            audio_path: None,
            source: "hoshiro".into(),
            words: 0,
        }
    }
}

pub fn now_ts() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Normalise a timestamp (RFC 3339 or "YYYY-MM-DD HH:MM:SS") to UTC RFC 3339 with millis.
pub fn normalize_ts(s: &str) -> Option<String> {
    let s = s.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc).to_rfc3339_opts(SecondsFormat::Millis, true));
    }
    for fmt in ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S%.f"] {
        if let Ok(n) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(n.and_utc().to_rfc3339_opts(SecondsFormat::Millis, true));
        }
    }
    None
}

const HISTORY_COLS: &str = "id, created_at, mode, status, raw_text, refined_text, selected_text, duration_secs, \
     app_name, bundle_id, window_title, error_message, audio_path, source, words";

fn row_to_history(r: &Row<'_>) -> rusqlite::Result<HistoryRow> {
    Ok(HistoryRow {
        id: r.get(0)?,
        created_at: r.get(1)?,
        mode: Mode::from_str(&r.get::<_, String>(2)?),
        status: r.get(3)?,
        raw_text: r.get(4)?,
        refined_text: r.get(5)?,
        selected_text: r.get(6)?,
        duration_secs: r.get::<_, Option<f64>>(7)?.unwrap_or(0.0),
        app_name: r.get(8)?,
        bundle_id: r.get(9)?,
        window_title: r.get(10)?,
        error_message: r.get(11)?,
        audio_path: r.get(12)?,
        source: r.get::<_, Option<String>>(13)?.unwrap_or_else(|| "hoshiro".into()),
        words: r.get::<_, Option<i64>>(14)?.unwrap_or(0).max(0) as u32,
    })
}

fn row_to_word(r: &Row<'_>) -> rusqlite::Result<DictionaryWord> {
    Ok(DictionaryWord { id: r.get(0)?, word: r.get(1)?, source: r.get(2)?, created_at: r.get(3)? })
}

fn migrate(conn: &Connection) -> anyhow::Result<()> {
    let mut version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 1 {
        conn.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS history (
                id TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                mode TEXT NOT NULL,
                status TEXT NOT NULL,
                raw_text TEXT,
                refined_text TEXT,
                selected_text TEXT,
                duration_secs REAL NOT NULL DEFAULT 0,
                app_name TEXT,
                bundle_id TEXT,
                window_title TEXT,
                error_message TEXT,
                audio_path TEXT,
                source TEXT NOT NULL DEFAULT 'hoshiro',
                words INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS idx_history_created_at ON history(created_at);
             CREATE INDEX IF NOT EXISTS idx_history_mode ON history(mode);
             CREATE TABLE IF NOT EXISTS dictionary (
                id TEXT PRIMARY KEY,
                word TEXT NOT NULL UNIQUE COLLATE NOCASE,
                source TEXT NOT NULL,
                created_at TEXT NOT NULL
             );
             PRAGMA user_version = 1;
             COMMIT;",
        )?;
        version = 1;
    }
    debug_assert_eq!(version, SCHEMA_VERSION);
    Ok(())
}

impl Db {
    pub fn open(path: &Path, recordings_dir: PathBuf) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::create_dir_all(&recordings_dir)?;
        let conn = Connection::open(path).with_context(|| format!("open database {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate(&conn)?;
        Ok(Self { conn: Mutex::new(conn), recordings_dir })
    }

    #[cfg(test)]
    pub fn open_in_memory(recordings_dir: PathBuf) -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        migrate(&conn)?;
        Ok(Self { conn: Mutex::new(conn), recordings_dir })
    }

    pub fn recordings_dir(&self) -> &Path {
        &self.recordings_dir
    }

    pub fn audio_file(&self, row: &HistoryRow) -> Option<PathBuf> {
        let p = self.recordings_dir.join(row.audio_path.as_ref()?);
        p.is_file().then_some(p)
    }

    pub fn to_item(&self, row: &HistoryRow) -> HistoryItem {
        HistoryItem {
            id: row.id.clone(),
            created_at: row.created_at.clone(),
            mode: row.mode,
            status: row.status.clone(),
            raw_text: row.raw_text.clone(),
            refined_text: row.refined_text.clone(),
            selected_text: row.selected_text.clone(),
            duration_secs: row.duration_secs,
            app_name: row.app_name.clone(),
            bundle_id: row.bundle_id.clone(),
            window_title: row.window_title.clone(),
            error_message: row.error_message.clone(),
            has_audio: self.audio_file(row).is_some(),
            source: row.source.clone(),
        }
    }

    // ───────── history ─────────

    pub fn upsert_history(&self, row: &HistoryRow) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        upsert(&conn, row)
    }

    pub fn get_history(&self, id: &str) -> anyhow::Result<Option<HistoryRow>> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row(&format!("SELECT {HISTORY_COLS} FROM history WHERE id = ?1"), params![id], row_to_history)
            .optional()?)
    }

    pub fn list_history(&self, q: &HistoryQuery) -> anyhow::Result<Vec<HistoryRow>> {
        let conn = self.conn.lock();
        let mut sql = format!("SELECT {HISTORY_COLS} FROM history WHERE 1=1");
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(mode) = q.mode {
            sql.push_str(" AND mode = ?");
            args.push(mode.as_str().to_string().into());
        }
        if let Some(search) = q.search.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            sql.push_str(" AND (refined_text LIKE ? ESCAPE '\\' OR raw_text LIKE ? ESCAPE '\\')");
            let escaped = search.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
            let pat = format!("%{escaped}%");
            args.push(pat.clone().into());
            args.push(pat.into());
        }
        sql.push_str(" ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?");
        args.push((q.limit.clamp(1, 500) as i64).into());
        args.push((q.offset as i64).into());
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args), row_to_history)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn last_completed_text(&self) -> anyhow::Result<Option<String>> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row(
                "SELECT refined_text FROM history WHERE status = 'completed' AND refined_text IS NOT NULL \
                 AND TRIM(refined_text) <> '' ORDER BY created_at DESC LIMIT 1",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?)
    }

    #[cfg(test)]
    pub fn count_history(&self) -> anyhow::Result<u64> {
        let conn = self.conn.lock();
        Ok(conn.query_row("SELECT COUNT(*) FROM history", [], |r| r.get::<_, i64>(0))?.max(0) as u64)
    }

    /// Returns true if a row was removed.
    pub fn delete_history(&self, id: &str) -> anyhow::Result<bool> {
        let conn = self.conn.lock();
        let audio: Option<String> = conn
            .query_row("SELECT audio_path FROM history WHERE id = ?1", params![id], |r| r.get(0))
            .optional()?
            .flatten();
        let n = conn.execute("DELETE FROM history WHERE id = ?1", params![id])?;
        drop(conn);
        if let Some(a) = audio {
            self.remove_audio(&a);
        }
        Ok(n > 0)
    }

    pub fn delete_all_history(&self) -> anyhow::Result<usize> {
        let conn = self.conn.lock();
        let audio = collect_audio(&conn, "SELECT audio_path FROM history WHERE audio_path IS NOT NULL", [])?;
        let n = conn.execute("DELETE FROM history", [])?;
        drop(conn);
        for a in audio {
            self.remove_audio(&a);
        }
        Ok(n)
    }

    /// Delete rows (and their audio) older than `cutoff`. Fresh "processing" rows are left alone.
    pub fn purge_older_than(&self, cutoff: DateTime<Utc>) -> anyhow::Result<usize> {
        let cutoff_s = cutoff.to_rfc3339_opts(SecondsFormat::Millis, true);
        let grace = (Utc::now() - chrono::Duration::hours(1)).to_rfc3339_opts(SecondsFormat::Millis, true);
        let conn = self.conn.lock();
        let audio = collect_audio(
            &conn,
            "SELECT audio_path FROM history WHERE created_at < ?1 AND audio_path IS NOT NULL \
             AND (status <> 'processing' OR created_at < ?2)",
            params![cutoff_s, grace],
        )?;
        let n = conn.execute(
            "DELETE FROM history WHERE created_at < ?1 AND (status <> 'processing' OR created_at < ?2)",
            params![cutoff_s, grace],
        )?;
        drop(conn);
        for a in audio {
            self.remove_audio(&a);
        }
        Ok(n)
    }

    /// Rows left in "processing" by a previous run (crash/quit) become retryable errors.
    pub fn fail_stale_processing(&self) -> anyhow::Result<usize> {
        let conn = self.conn.lock();
        Ok(conn.execute(
            "UPDATE history SET status = 'error', error_message = '处理被中断' WHERE status = 'processing'",
            [],
        )?)
    }

    pub fn stat_rows(&self) -> anyhow::Result<Vec<StatRow>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT created_at, duration_secs, words FROM history WHERE status = 'completed'")?;
        let rows = stmt.query_map([], |r| {
            Ok(StatRow {
                created_at: r.get(0)?,
                duration_secs: r.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
                words: r.get::<_, Option<i64>>(2)?.unwrap_or(0).max(0) as u32,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Insert imported rows, skipping ids that already exist. Returns (imported, skipped).
    pub fn import_rows(&self, rows: &[HistoryRow]) -> anyhow::Result<(u32, u32)> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let (mut imported, mut skipped) = (0u32, 0u32);
        for r in rows {
            let exists: bool =
                tx.query_row("SELECT EXISTS(SELECT 1 FROM history WHERE id = ?1)", params![r.id], |x| x.get(0))?;
            if exists {
                skipped += 1;
                continue;
            }
            upsert(&tx, r)?;
            imported += 1;
        }
        tx.commit()?;
        Ok((imported, skipped))
    }

    fn remove_audio(&self, file_name: &str) {
        // Only ever delete plain file names inside the recordings dir.
        if file_name.contains('/') || file_name.contains("..") {
            return;
        }
        let p = self.recordings_dir.join(file_name);
        if let Err(e) = std::fs::remove_file(&p) {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("cannot remove audio {}: {e}", p.display());
            }
        }
    }

    // ───────── dictionary ─────────

    pub fn list_words(&self) -> anyhow::Result<Vec<DictionaryWord>> {
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare("SELECT id, word, source, created_at FROM dictionary ORDER BY created_at DESC, word ASC")?;
        let rows = stmt.query_map([], row_to_word)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Words for LLM prompts: most recently added first.
    pub fn words_for_prompt(&self, limit: usize) -> anyhow::Result<Vec<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT word FROM dictionary ORDER BY created_at DESC, word ASC LIMIT ?1")?;
        let rows = stmt.query_map(params![limit as i64], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Adds words that are not present yet (case-insensitive); returns the newly created entries.
    pub fn add_words(&self, words: &[String], source: &str) -> anyhow::Result<Vec<DictionaryWord>> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let mut added = Vec::new();
        let base = Utc::now();
        for (i, w) in words.iter().enumerate() {
            let entry = DictionaryWord {
                id: uuid::Uuid::new_v4().to_string(),
                word: w.clone(),
                source: source.to_string(),
                // keep insertion order stable for words added in one batch
                created_at: (base + chrono::Duration::milliseconds(i as i64))
                    .to_rfc3339_opts(SecondsFormat::Millis, true),
            };
            let n = tx.execute(
                "INSERT OR IGNORE INTO dictionary (id, word, source, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![entry.id, entry.word, entry.source, entry.created_at],
            )?;
            if n > 0 {
                added.push(entry);
            }
        }
        tx.commit()?;
        Ok(added)
    }

    pub fn update_word(&self, id: &str, word: &str) -> anyhow::Result<Option<DictionaryWord>> {
        let conn = self.conn.lock();
        let n = conn.execute("UPDATE dictionary SET word = ?2 WHERE id = ?1", params![id, word])?;
        if n == 0 {
            return Ok(None);
        }
        Ok(conn
            .query_row("SELECT id, word, source, created_at FROM dictionary WHERE id = ?1", params![id], row_to_word)
            .optional()?)
    }

    pub fn delete_words(&self, ids: &[String]) -> anyhow::Result<usize> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let mut n = 0;
        for id in ids {
            n += tx.execute("DELETE FROM dictionary WHERE id = ?1", params![id])?;
        }
        tx.commit()?;
        Ok(n)
    }
}

fn collect_audio(conn: &Connection, sql: &str, p: impl rusqlite::Params) -> anyhow::Result<Vec<String>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(p, |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn upsert(conn: &Connection, row: &HistoryRow) -> anyhow::Result<()> {
    conn.execute(
        &format!(
            "INSERT OR REPLACE INTO history ({HISTORY_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)"
        ),
        params![
            row.id,
            row.created_at,
            row.mode.as_str(),
            row.status,
            row.raw_text,
            row.refined_text,
            row.selected_text,
            row.duration_secs,
            row.app_name,
            row.bundle_id,
            row.window_title,
            row.error_message,
            row.audio_path,
            row.source,
            row.words as i64,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("hoshiro-db-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn row(id: &str, mode: Mode, status: &str, text: &str, ts: &str) -> HistoryRow {
        let mut r = HistoryRow::new(id.into(), mode, status);
        r.created_at = ts.into();
        r.refined_text = Some(text.into());
        r.raw_text = Some(format!("raw {text}"));
        r.words = crate::stats::count_words(text);
        r
    }

    fn q(mode: Option<Mode>, search: Option<&str>) -> HistoryQuery {
        HistoryQuery { mode, limit: 50, offset: 0, search: search.map(String::from) }
    }

    #[test]
    fn history_roundtrip_and_filters() {
        let dir = tmp_dir();
        let db = Db::open_in_memory(dir.clone()).unwrap();
        db.upsert_history(&row("a", Mode::Dictation, "completed", "hello world", "2026-10-01T10:00:00.000Z")).unwrap();
        db.upsert_history(&row("b", Mode::Translation, "completed", "你好 100%", "2026-10-02T10:00:00.000Z")).unwrap();
        db.upsert_history(&row("c", Mode::Ask, "error", "x", "2026-10-03T10:00:00.000Z")).unwrap();

        let all = db.list_history(&q(None, None)).unwrap();
        assert_eq!(all.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["c", "b", "a"]);
        assert_eq!(db.list_history(&q(Some(Mode::Translation), None)).unwrap().len(), 1);
        assert_eq!(db.list_history(&q(None, Some("hello"))).unwrap().len(), 1);
        assert_eq!(db.list_history(&q(None, Some("100%"))).unwrap().len(), 1);
        assert_eq!(db.list_history(&q(None, Some("%"))).unwrap().len(), 1);
        assert_eq!(db.last_completed_text().unwrap().as_deref(), Some("你好 100%"));

        let got = db.get_history("a").unwrap().unwrap();
        assert_eq!(got.mode, Mode::Dictation);
        assert_eq!(got.words, 2);
        assert!(db.delete_history("a").unwrap());
        assert!(!db.delete_history("a").unwrap());
        assert_eq!(db.count_history().unwrap(), 2);
        db.delete_all_history().unwrap();
        assert_eq!(db.count_history().unwrap(), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn purge_removes_old_rows_and_audio() {
        let dir = tmp_dir();
        let db = Db::open_in_memory(dir.clone()).unwrap();
        let mut old = row("old", Mode::Dictation, "completed", "old", "2020-01-01T00:00:00.000Z");
        old.audio_path = Some("old.wav".into());
        std::fs::write(dir.join("old.wav"), b"x").unwrap();
        let fresh = row("fresh", Mode::Dictation, "completed", "fresh", &now_ts());
        db.upsert_history(&old).unwrap();
        db.upsert_history(&fresh).unwrap();
        assert!(db.to_item(&old).has_audio);

        let n = db.purge_older_than(Utc::now() - chrono::Duration::days(30)).unwrap();
        assert_eq!(n, 1);
        assert!(!dir.join("old.wav").exists());
        assert_eq!(db.count_history().unwrap(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_processing_becomes_error() {
        let db = Db::open_in_memory(tmp_dir()).unwrap();
        db.upsert_history(&row("p", Mode::Dictation, "processing", "", &now_ts())).unwrap();
        assert_eq!(db.fail_stale_processing().unwrap(), 1);
        assert_eq!(db.get_history("p").unwrap().unwrap().status, "error");
    }

    #[test]
    fn dictionary_is_case_insensitive_unique() {
        let db = Db::open_in_memory(tmp_dir()).unwrap();
        let added = db.add_words(&["Typeless".into(), "typeless".into(), "MiMo".into()], "manual").unwrap();
        assert_eq!(added.len(), 2);
        let again = db.add_words(&["TYPELESS".into()], "auto").unwrap();
        assert!(again.is_empty());
        assert_eq!(db.list_words().unwrap().len(), 2);

        let id = added[0].id.clone();
        let upd = db.update_word(&id, "Hoshiro").unwrap().unwrap();
        assert_eq!(upd.word, "Hoshiro");
        assert!(db.update_word(&id, "mimo").is_err());
        assert!(db.update_word("nope", "x").unwrap().is_none());
        assert_eq!(db.delete_words(&[id]).unwrap(), 1);
        assert_eq!(db.words_for_prompt(10).unwrap(), vec!["MiMo".to_string()]);
    }

    #[test]
    fn import_skips_existing() {
        let db = Db::open_in_memory(tmp_dir()).unwrap();
        db.upsert_history(&row("a", Mode::Dictation, "completed", "one", "2026-10-01T10:00:00.000Z")).unwrap();
        let mut b = row("b", Mode::Dictation, "completed", "two", "2026-10-02T10:00:00.000Z");
        b.source = "typeless".into();
        let a2 = row("a", Mode::Dictation, "completed", "dup", "2026-10-01T10:00:00.000Z");
        let (i, s) = db.import_rows(&[a2, b]).unwrap();
        assert_eq!((i, s), (1, 1));
        assert_eq!(db.get_history("a").unwrap().unwrap().refined_text.as_deref(), Some("one"));
        assert_eq!(db.stat_rows().unwrap().len(), 2);
    }

    #[test]
    fn migrations_on_disk_are_idempotent() {
        let dir = tmp_dir();
        let p = dir.join("t.db");
        {
            let db = Db::open(&p, dir.join("rec")).unwrap();
            db.upsert_history(&row("a", Mode::Dictation, "completed", "x", &now_ts())).unwrap();
        }
        let db = Db::open(&p, dir.join("rec")).unwrap();
        assert_eq!(db.count_history().unwrap(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn timestamp_normalisation() {
        assert_eq!(normalize_ts("2026-10-09T18:57:31.104Z").unwrap(), "2026-10-09T18:57:31.104Z");
        assert_eq!(normalize_ts("2026-10-09T20:57:31+02:00").unwrap(), "2026-10-09T18:57:31.000Z");
        assert_eq!(normalize_ts("2026-10-09 18:57:31").unwrap(), "2026-10-09T18:57:31.000Z");
        assert!(normalize_ts("garbage").is_none());
    }
}
