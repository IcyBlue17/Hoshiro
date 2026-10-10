//! Evaluate the 30 most recent real dictations. No fixtures masquerading as user history.
//! cargo run --example rewrite_eval -- <app-data-dir> [--dry-run] > comparison.jsonl
use anyhow::Context;
use hoshiro_lib::{
    platform::AppContext,
    providers::{self, RewriteOutcome, RewriteRequest},
    types::{Mode, Settings},
};
use rusqlite::Connection;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let dir = PathBuf::from(args.first().context("用法：rewrite_eval <应用数据目录> [--dry-run]")?);
    let settings: Settings = serde_json::from_slice(&std::fs::read(dir.join("settings.json"))?)?;
    let db = Connection::open_with_flags(dir.join("hoshiro.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = db.prepare("SELECT id, raw_text, duration_secs, app_name, refined_text FROM history WHERE status='completed' AND mode='dictation' AND raw_text IS NOT NULL ORDER BY created_at DESC LIMIT 30")?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut words = db.prepare("SELECT word FROM dictionary ORDER BY created_at DESC LIMIT 200")?;
    let dictionary = words.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    eprintln!("评测 {} 条真实记录；输出含私人文本，请勿提交到仓库。", rows.len());
    for (id, raw, duration_secs, app_name, previous) in rows {
        let req = RewriteRequest {
            mode: Mode::Dictation,
            transcript: raw.clone(),
            selected_text: None,
            context: AppContext { app_name, ..Default::default() },
            dictionary: dictionary.clone(),
            languages: settings.selected_languages.clone(),
            translation_target: None,
            custom_instructions: settings.style.custom_instructions.clone(),
            smart_formatting: settings.style.smart_formatting,
            long_lists: false,
            duration_secs,
            profile: None,
        };
        let start = std::time::Instant::now();
        let result = if args.iter().any(|a| a == "--dry-run") {
            serde_json::json!({"prompt": providers::prompts::build_user_message(&req)})
        } else {
            match providers::rewrite(&settings.llm, &req).await {
                Ok(RewriteOutcome::Insert(text) | RewriteOutcome::Replace(text) | RewriteOutcome::Answer(text)) => {
                    serde_json::json!({"after":text})
                }
                Err(e) => serde_json::json!({"error":e.to_string()}),
            }
        };
        println!(
            "{}",
            serde_json::json!({"id":id,"raw":raw,"previous":previous,"durationSecs":duration_secs,"elapsedMs":start.elapsed().as_millis(),"result":result})
        );
    }
    Ok(())
}
