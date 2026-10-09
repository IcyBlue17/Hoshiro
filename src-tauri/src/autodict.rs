//! Auto-dictionary: detect a single corrected token between the text we inserted and what the
//! field contains a few seconds later.

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x3040..=0x30FF | 0xAC00..=0xD7AF)
}

fn is_latin_word(c: char) -> bool {
    c.is_alphanumeric() && !is_cjk(c)
}

const COMMON: &[&str] = &[
    "the", "be", "to", "of", "and", "a", "in", "that", "have", "i", "it", "for", "not", "on", "with", "he", "as", "you",
    "do", "at", "this", "but", "his", "by", "from", "they", "we", "say", "her", "she", "or", "an", "will", "my", "one",
    "all", "would", "there", "their", "what", "so", "up", "out", "if", "about", "who", "get", "which", "go", "me",
    "when", "make", "can", "like", "time", "no", "just", "him", "know", "take", "people", "into", "year", "your",
    "good", "some", "could", "them", "see", "other", "than", "then", "now", "look", "only", "come", "its", "over",
    "think", "also", "back", "after", "use", "two", "how", "our", "work", "first", "well", "way", "even", "new",
    "want", "because", "any", "these", "give", "day", "most", "us", "is", "are", "was", "were", "been", "has", "had",
    "did", "does", "am", "very", "too", "here", "yes", "ok", "okay", "hi", "hello", "thanks", "please", "should",
    "need", "let", "put", "set", "run", "cat", "dog", "red", "big", "end", "off", "own", "may", "why", "where",
];

fn is_common(w: &str) -> bool {
    let l = w.to_lowercase();
    COMMON.contains(&l.as_str())
}

pub fn edit_distance(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

fn find_sub(hay: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

/// Locate only an unambiguous anchored region; never learn surrounding field content.
pub fn edited_region(inserted: &str, field: &str) -> Option<String> {
    let ins: Vec<char> = inserted.trim().chars().collect();
    let fld: Vec<char> = field.chars().collect();
    if ins.len() < 6 { return None; }
    let k = (ins.len() / 3).clamp(2, 8);
    let start = find_sub(&fld, &ins[..k], 0)?;
    if find_sub(&fld, &ins[..k], start + 1).is_some() { return None; }
    let tail = &ins[ins.len() - k..];
    let end = find_sub(&fld, tail, start + k)?;
    if find_sub(&fld, tail, end + 1).is_some() { return None; }
    let region = &fld[start..end + k];
    if region.len() > ins.len() * 2 + 100 { return None; }
    Some(region.iter().collect())
}

/// If `field` contains `inserted` with exactly one token replaced by a similar different token,
/// return the new token.
pub fn detect_correction(inserted: &str, field: &str) -> Option<String> {
    let ins: Vec<char> = inserted.trim().chars().collect();
    let fld: Vec<char> = field.chars().collect();
    if ins.len() < 6 || fld.len() < 6 {
        return None;
    }
    let k = (ins.len() / 3).clamp(2, 5);
    let start = find_sub(&fld, &ins[..k], 0)?;
    let end_anchor = &ins[ins.len() - k..];
    let end = find_sub(&fld, end_anchor, start)? + k;
    if end <= start {
        return None;
    }
    let region = &fld[start..end];
    if region.len().abs_diff(ins.len()) > 24 || region == &ins[..] {
        return None;
    }

    let p = ins.iter().zip(region.iter()).take_while(|(a, b)| a == b).count();
    let (ins_rest, reg_rest) = (&ins[p..], &region[p..]);
    let s = ins_rest.iter().rev().zip(reg_rest.iter().rev()).take_while(|(a, b)| a == b).count();
    let old_mid = &ins_rest[..ins_rest.len() - s];
    let new_mid = &reg_rest[..reg_rest.len() - s];
    if old_mid.is_empty() && new_mid.is_empty() {
        return None;
    }

    // Expand to whole tokens.
    let latin = new_mid.iter().chain(old_mid.iter()).all(|c| is_latin_word(*c));
    let (mut a, mut b_old, mut b_new) = (p, ins.len() - s, region.len() - s);
    if latin {
        while a > 0 && is_latin_word(ins[a - 1]) {
            a -= 1;
        }
        while b_old < ins.len() && is_latin_word(ins[b_old]) {
            b_old += 1;
        }
        while b_new < region.len() && is_latin_word(region[b_new]) {
            b_new += 1;
        }
    } else {
        if old_mid.is_empty() || new_mid.is_empty() {
            return None;
        }
        if new_mid.iter().all(|c| is_cjk(*c)) && new_mid.len() < 2 && a > 0 && is_cjk(ins[a - 1]) {
            a -= 1;
        }
    }
    let old_tok: Vec<char> = ins[a..b_old].to_vec();
    let new_tok: Vec<char> = region[a..b_new].to_vec();

    if old_tok == new_tok || new_tok.len() < 2 || new_tok.len() > 32 || old_tok.is_empty() {
        return None;
    }
    if new_tok.iter().chain(old_tok.iter()).any(|c| c.is_whitespace()) {
        return None;
    }
    if !new_tok.iter().any(|c| c.is_alphabetic()) {
        return None;
    }
    if edit_distance(&old_tok, &new_tok) > 3 {
        return None;
    }
    let tok: String = new_tok.iter().collect();
    if is_common(&tok) {
        return None;
    }
    Some(tok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchored_edits_do_not_capture_unrelated_fields() {
        assert_eq!(edited_region("请把今天安排的三项任务做完之后发给我", "前文。请把今天安排的两项任务做完之后发给我。后文").as_deref(), Some("请把今天安排的两项任务做完之后发给我"));
        assert!(edited_region("please open the dashboard now", "unrelated field").is_none());
        assert!(edited_region("please open the dashboard now", "please open the dashboard now; please open the dashboard now").is_none());
    }

    #[test]
    fn single_token_fix() {
        let r = detect_correction("I use Hoshira to dictate every day", "I use Hoshiro to dictate every day");
        assert_eq!(r.as_deref(), Some("Hoshiro"));
    }

    #[test]
    fn with_surrounding_text() {
        let r = detect_correction(
            "please open the Kubernets dashboard now",
            "Dear team,\nplease open the Kubernetes dashboard now\nthanks",
        );
        assert_eq!(r.as_deref(), Some("Kubernetes"));
    }

    #[test]
    fn case_fix() {
        let r = detect_correction("I bought a new iphone yesterday", "I bought a new iPhone yesterday");
        assert_eq!(r.as_deref(), Some("iPhone"));
    }

    #[test]
    fn unchanged_or_multiple_changes() {
        assert!(detect_correction("hello there my friend", "hello there my friend").is_none());
        assert!(detect_correction("hello there my friend", "hello where my fiend").is_none());
        assert!(detect_correction("hello there my friend", "something totally different").is_none());
    }

    #[test]
    fn far_or_common_replacements_are_ignored() {
        assert!(detect_correction("I saw a dog yesterday there", "I saw a cat yesterday there").is_none());
        assert!(detect_correction("send it to Alexander today", "send it to Bob today").is_none());
    }

    #[test]
    fn cjk_fix() {
        let r = detect_correction("我在用星城输入法写东西", "我在用星辰输入法写东西");
        assert_eq!(r.as_deref(), Some("星辰"));
    }

    #[test]
    fn deletion_only_is_ignored() {
        assert!(detect_correction("hello there my dear friend", "hello there my friend").is_none());
    }
}
