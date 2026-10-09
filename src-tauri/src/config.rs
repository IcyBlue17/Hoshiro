//! Settings persistence: JSON at `<data dir>/settings.json` (0600, atomic writes, defaults for missing fields).

use crate::types::{Settings, ShortcutBindings};
use anyhow::Context;
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const RETENTION_VALUES: [&str; 6] = ["never", "24h", "1w", "1m", "1y", "forever"];

pub fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join("settings.json")
}

/// Load settings; never fails. A corrupt file is moved aside and defaults are used.
pub fn load(data_dir: &Path) -> Settings {
    let path = settings_path(data_dir);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("cannot read {}: {e}", path.display());
            }
            return Settings::default();
        }
    };
    match parse_settings(&text) {
        Ok(mut s) => {
            sanitize(&mut s);
            s
        }
        Err(e) => {
            log::error!("settings.json is unreadable ({e:#}); backing it up and using defaults");
            let _ = std::fs::rename(&path, data_dir.join("settings.json.bak"));
            Settings::default()
        }
    }
}

/// Parse a (possibly partial) settings JSON document; missing fields take their defaults and
/// individual fields with a wrong type are skipped instead of invalidating the whole file.
pub fn parse_settings(text: &str) -> anyhow::Result<Settings> {
    let loaded: Value = serde_json::from_str(text).context("invalid JSON")?;
    let Value::Object(loaded_map) = &loaded else {
        anyhow::bail!("settings root is not an object");
    };
    let defaults = serde_json::to_value(Settings::default())?;
    let mut merged = defaults.clone();
    merge(&mut merged, loaded.clone());
    if let Ok(s) = serde_json::from_value::<Settings>(merged) {
        return Ok(s);
    }
    // Field-by-field recovery.
    let mut acc = defaults.clone();
    for (k, v) in loaded_map {
        let mut candidate = acc.clone();
        if let Value::Object(m) = &mut candidate {
            let mut piece = m.get(k).cloned().unwrap_or(Value::Null);
            merge(&mut piece, v.clone());
            m.insert(k.clone(), piece);
        }
        if serde_json::from_value::<Settings>(candidate.clone()).is_ok() {
            acc = candidate;
        } else {
            log::warn!("ignoring invalid settings field '{k}'");
        }
    }
    serde_json::from_value(acc).context("settings recovery failed")
}

fn merge(base: &mut Value, over: Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(slot) => merge(slot, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (_, Value::Null) => {}
        (slot, v) => *slot = v,
    }
}

pub fn sanitize(s: &mut Settings) {
    let d = Settings::default();
    let trim_all = |v: &mut Vec<String>| {
        for x in v.iter_mut() {
            *x = x.trim().to_string();
        }
        v.retain(|x| !x.is_empty());
        let mut seen = std::collections::HashSet::new();
        v.retain(|x| seen.insert(x.clone()));
    };
    trim_all(&mut s.shortcuts.dictation);
    trim_all(&mut s.shortcuts.ask_anything);
    trim_all(&mut s.shortcuts.translation);
    trim_all(&mut s.shortcuts.paste_last_transcript);
    if s.shortcuts.dictation.is_empty() {
        s.shortcuts.dictation = ShortcutBindings::default().dictation;
    }
    trim_all(&mut s.selected_languages);
    if s.selected_languages.is_empty() {
        s.selected_languages = d.selected_languages.clone();
    }
    trim_all(&mut s.translation_targets);
    s.translation_targets.truncate(3);
    if s.translation_targets.is_empty() {
        s.translation_targets = d.translation_targets.clone();
    }
    if !RETENTION_VALUES.contains(&s.history_retention.as_str()) {
        s.history_retention = d.history_retention.clone();
    }
    if !["system", "light", "dark"].contains(&s.appearance.as_str()) {
        s.appearance = "system".into();
    }
    if !["zh-CN", "en-US"].contains(&s.interface_language.as_str()) {
        s.interface_language = "zh-CN".into();
    }
    if let Some(id) = &s.microphone_device_id {
        if id.trim().is_empty() || id == "default" || id == "auto" {
            s.microphone_device_id = None;
        }
    }

    if !["mimo", "cloudflare", "azure", "google"].contains(&s.asr.provider.as_str()) {
        s.asr.provider = "mimo".into();
    }
    for value in [&mut s.asr.cloudflare.account_id, &mut s.asr.cloudflare.api_key,
        &mut s.asr.cloudflare.model, &mut s.asr.azure.api_key, &mut s.asr.azure.region,
        &mut s.asr.google.api_key, &mut s.asr.google.service_account_json,
        &mut s.asr.google.project_id, &mut s.asr.google.location, &mut s.asr.google.model] {
        *value = value.trim().to_string();
    }

    s.asr.mimo.api_key = s.asr.mimo.api_key.trim().to_string();
    s.asr.mimo.base_url = s.asr.mimo.base_url.trim().trim_end_matches('/').to_string();
    s.asr.mimo.model = s.asr.mimo.model.trim().to_string();
    if s.asr.mimo.base_url.is_empty() {
        s.asr.mimo.base_url = d.asr.mimo.base_url.clone();
    }
    if s.asr.mimo.model.is_empty() {
        s.asr.mimo.model = d.asr.mimo.model.clone();
    }
    for (ep, def) in [
        (&mut s.llm.gemini, &d.llm.gemini),
        (&mut s.llm.anthropic, &d.llm.anthropic),
        (&mut s.llm.openai, &d.llm.openai),
    ] {
        ep.api_key = ep.api_key.trim().to_string();
        ep.base_url = ep.base_url.trim().trim_end_matches('/').to_string();
        ep.model = ep.model.trim().to_string();
        if ep.model.is_empty() {
            ep.model = def.model.clone();
        }
    }
    if s.llm.timeout_secs == 0 {
        s.llm.timeout_secs = d.llm.timeout_secs;
    }
    s.llm.timeout_secs = s.llm.timeout_secs.clamp(3, 120);
}

/// Atomic write with 0600 permissions (the file contains API keys).
pub fn save(data_dir: &Path, s: &Settings) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir).context("create data dir")?;
    let path = settings_path(data_dir);
    let tmp = data_dir.join("settings.json.tmp");
    let json = serde_json::to_vec_pretty(s)?;
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp).context("open temp settings file")?;
        f.write_all(&json)?;
        f.sync_all()?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, &path).context("replace settings file")?;
    Ok(())
}

pub fn asr_language(s: &Settings) -> &'static str {
    crate::providers::language_param(&s.selected_languages, s.auto_select_languages)
}

pub fn language_label(code: &str) -> String {
    let name = match code {
        "zh-CN" => "简体中文",
        "zh-TW" => "繁體中文",
        "zh-HK" => "粵語",
        "en-US" | "en-GB" | "en-AU" | "en-CA" | "en" => "English",
        "ja-JP" | "ja" => "日本語",
        "ko-KR" | "ko" => "한국어",
        "fr-FR" | "fr" => "Français",
        "de-DE" | "de" => "Deutsch",
        "es-ES" | "es-MX" | "es" => "Español",
        "pt-BR" | "pt-PT" | "pt" => "Português",
        "ru-RU" | "ru" => "Русский",
        "it-IT" | "it" => "Italiano",
        "vi-VN" | "vi" => "Tiếng Việt",
        "th-TH" | "th" => "ไทย",
        "id-ID" | "id" => "Bahasa Indonesia",
        "ar-SA" | "ar" => "العربية",
        "hi-IN" | "hi" => "हिन्दी",
        "tr-TR" | "tr" => "Türkçe",
        "nl-NL" | "nl" => "Nederlands",
        "pl-PL" | "pl" => "Polski",
        "uk-UA" | "uk" => "Українська",
        _ => code,
    };
    name.to_string()
}

/// Seconds a given retention keeps history for; None = forever, Some(0) = never store.
pub fn retention_secs(retention: &str) -> Option<i64> {
    match retention {
        "never" => Some(0),
        "24h" => Some(86_400),
        "1w" => Some(7 * 86_400),
        "1m" => Some(30 * 86_400),
        "1y" => Some(365 * 86_400),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_file_gets_defaults() {
        let s = parse_settings(r#"{"launchAtLogin": false, "shortcuts": {"dictation": ["Help"]}}"#).unwrap();
        assert!(!s.launch_at_login);
        assert_eq!(s.shortcuts.dictation, vec!["Help".to_string()]);
        assert_eq!(s.shortcuts.ask_anything, vec!["Fn+Space".to_string()]);
        assert_eq!(s.llm.timeout_secs, 12);
    }

    #[test]
    fn wrong_typed_field_is_skipped() {
        let s = parse_settings(r#"{"launchAtLogin": "yes", "showInDock": false, "asr": {"mimo": {"apiKey": "k"}}}"#).unwrap();
        assert!(s.launch_at_login);
        assert!(!s.show_in_dock);
        assert_eq!(s.asr.mimo.api_key, "k");
        assert_eq!(s.asr.mimo.model, "mimo-v2.5-asr");
    }

    #[test]
    fn sanitize_fixes_bad_values() {
        let mut s = Settings::default();
        s.shortcuts.dictation = vec![];
        s.history_retention = "bogus".into();
        s.translation_targets = vec!["a".into(), "b".into(), "c".into(), "d".into()];
        s.asr.mimo.api_key = "  key\n".into();
        s.microphone_device_id = Some("default".into());
        sanitize(&mut s);
        assert_eq!(s.shortcuts.dictation, vec!["Fn".to_string()]);
        assert_eq!(s.history_retention, "1m");
        assert_eq!(s.translation_targets.len(), 3);
        assert_eq!(s.asr.mimo.api_key, "key");
        assert!(s.microphone_device_id.is_none());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("hoshiro-cfg-{}", uuid::Uuid::new_v4()));
        let mut s = Settings::default();
        s.asr.mimo.api_key = "secret".into();
        save(&dir, &s).unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded.asr.mimo.api_key, "secret");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(settings_path(&dir)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn asr_language_selection() {
        let mut s = Settings::default();
        assert_eq!(asr_language(&s), "zh");
        s.selected_languages = vec!["en-US".into(), "en-GB".into()];
        assert_eq!(asr_language(&s), "en");
        s.selected_languages = vec!["zh-CN".into(), "en-US".into()];
        assert_eq!(asr_language(&s), "auto");
    }
}
