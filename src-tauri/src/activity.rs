//! "Today's activity" for the active account: estimated cost, turns and lines written, derived from
//! Claude Code's local session transcripts (`<config dir>/projects/**/*.jsonl`).
//!
//! Cost is an *estimate* at public API list prices — a subscription is not billed per token.

use crate::model::{SwitchEvent, TodayView};
use chrono::{DateTime, Local, TimeZone};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Clone, Debug)]
pub struct Entry {
    pub ts_ms: i64,
    pub family: String,
    pub cost: f64,
    pub turn: bool,
    pub lines: u32,
}

#[derive(Default)]
pub struct Scanner {
    cache: HashMap<PathBuf, (u64, u64, Vec<Entry>)>,
}

pub fn local_midnight_ms() -> i64 {
    let midnight = Local::now().date_naive().and_hms_opt(0, 0, 0).unwrap();
    Local
        .from_local_datetime(&midnight)
        .earliest()
        .map(|d| d.timestamp_millis())
        .unwrap_or(0)
}

impl Scanner {
    /// All entries since `since_ms` from every transcript touched since then.
    pub fn scan(&mut self, config_dir: &Path, since_ms: i64) -> Vec<Entry> {
        let mut files = Vec::new();
        collect_jsonl(&config_dir.join("projects"), 0, since_ms, &mut files);
        let live: HashSet<&PathBuf> = files.iter().map(|(p, _, _)| p).collect();
        self.cache.retain(|p, _| live.contains(p));

        let mut out = Vec::new();
        for (path, mtime, size) in &files {
            let fresh = matches!(self.cache.get(path), Some((m, s, _)) if m == mtime && s == size);
            if !fresh {
                let entries = parse_file(path);
                self.cache.insert(path.clone(), (*mtime, *size, entries));
            }
            if let Some((_, _, entries)) = self.cache.get(path) {
                out.extend(entries.iter().filter(|e| e.ts_ms >= since_ms).cloned());
            }
        }
        out
    }
}

fn collect_jsonl(dir: &Path, depth: u8, since_ms: i64, out: &mut Vec<(PathBuf, u64, u64)>) {
    if depth > 4 {
        return;
    }
    let Ok(read) = fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            collect_jsonl(&path, depth + 1, since_ms, out);
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            if mtime as i64 >= since_ms {
                out.push((path, mtime, meta.len()));
            }
        }
    }
}

fn parse_file(path: &Path) -> Vec<Entry> {
    let Ok(text) = fs::read_to_string(path) else { return Vec::new() };
    // one API response is written as several lines sharing a message id; keep the fullest usage
    let mut messages: HashMap<String, (i64, String, f64, u64)> = HashMap::new();
    let mut seen_tools: HashSet<String> = HashSet::new();
    let mut entries = Vec::new();

    for line in text.lines() {
        if line.len() < 20 || !line.starts_with('{') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let Some(ts) = v
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.timestamp_millis())
        else {
            continue;
        };
        match v.get("type").and_then(Value::as_str) {
            Some("user") => {
                if v.get("isSidechain").and_then(Value::as_bool) == Some(true)
                    || v.get("isMeta").and_then(Value::as_bool) == Some(true)
                {
                    continue;
                }
                let content = v.pointer("/message/content");
                let is_prompt = match content {
                    Some(Value::String(s)) => !s.trim().is_empty(),
                    Some(Value::Array(blocks)) => blocks
                        .iter()
                        .any(|b| b.get("type").and_then(Value::as_str) == Some("text")),
                    _ => false,
                };
                if is_prompt {
                    entries.push(Entry { ts_ms: ts, family: String::new(), cost: 0.0, turn: true, lines: 0 });
                }
            }
            Some("assistant") => {
                let Some(msg) = v.get("message") else { continue };
                let model = msg.get("model").and_then(Value::as_str).unwrap_or_default();
                if model.is_empty() || model == "<synthetic>" {
                    continue;
                }
                if let Some(usage) = msg.get("usage") {
                    let id = msg
                        .get("id")
                        .and_then(Value::as_str)
                        .or_else(|| v.get("requestId").and_then(Value::as_str))
                        .unwrap_or_default()
                        .to_string();
                    let out_tokens = usage.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
                    let cost = cost_of(model, usage);
                    if !id.is_empty() {
                        let slot = messages.entry(id).or_insert((ts, model.to_string(), 0.0, 0));
                        if out_tokens >= slot.3 {
                            *slot = (ts, model.to_string(), cost, out_tokens);
                        }
                    } else {
                        entries.push(Entry { ts_ms: ts, family: family_of(model), cost, turn: false, lines: 0 });
                    }
                }
                if let Some(blocks) = msg.get("content").and_then(Value::as_array) {
                    for b in blocks {
                        if b.get("type").and_then(Value::as_str) != Some("tool_use") {
                            continue;
                        }
                        let tool_id = b.get("id").and_then(Value::as_str).unwrap_or_default();
                        if !tool_id.is_empty() && !seen_tools.insert(tool_id.to_string()) {
                            continue;
                        }
                        let lines = lines_written(b);
                        if lines > 0 {
                            entries.push(Entry { ts_ms: ts, family: String::new(), cost: 0.0, turn: false, lines });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for (_, (ts, model, cost, _)) in messages {
        entries.push(Entry { ts_ms: ts, family: family_of(&model), cost, turn: false, lines: 0 });
    }
    entries
}

fn count_lines(s: &str) -> u32 {
    if s.is_empty() { 0 } else { s.lines().count().max(1) as u32 }
}

fn lines_written(block: &Value) -> u32 {
    let input = block.get("input");
    match block.get("name").and_then(Value::as_str) {
        Some("Write") => input.and_then(|i| i.get("content")).and_then(Value::as_str).map(count_lines).unwrap_or(0),
        Some("Edit") => input.and_then(|i| i.get("new_string")).and_then(Value::as_str).map(count_lines).unwrap_or(0),
        Some("MultiEdit") => input
            .and_then(|i| i.get("edits"))
            .and_then(Value::as_array)
            .map(|edits| {
                edits
                    .iter()
                    .filter_map(|e| e.get("new_string").and_then(Value::as_str))
                    .map(count_lines)
                    .sum()
            })
            .unwrap_or(0),
        _ => 0,
    }
}

pub fn family_of(model: &str) -> String {
    let m = model.to_lowercase();
    for fam in ["opus", "sonnet", "haiku", "fable"] {
        if m.contains(fam) {
            let mut c = fam.chars();
            return c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
        }
    }
    model.trim_start_matches("claude-").to_string()
}

/// USD per million tokens (input, output). List prices; unknown models fall back to Sonnet pricing.
fn price_per_mtok(model: &str) -> (f64, f64) {
    let m = model.to_lowercase();
    if m.contains("opus") {
        if m.contains("opus-4-5") || m.contains("opus-4.5") || m.contains("opus-5") {
            (5.0, 25.0)
        } else {
            (15.0, 75.0)
        }
    } else if m.contains("haiku") {
        if m.contains("haiku-3") { (0.8, 4.0) } else { (1.0, 5.0) }
    } else if m.contains("fable") {
        (5.0, 25.0)
    } else {
        (3.0, 15.0)
    }
}

fn cost_of(model: &str, usage: &Value) -> f64 {
    let (pin, pout) = price_per_mtok(model);
    let n = |k: &str| usage.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    (n("input_tokens") * pin
        + n("output_tokens") * pout
        + n("cache_creation_input_tokens") * pin * 1.25
        + n("cache_read_input_tokens") * pin * 0.1)
        / 1_000_000.0
}

/// Who was the active account at `ts`? The latest switch at or before it; before the first logged
/// switch we assume the first logged account (it was already active).
fn owner_at(log: &[SwitchEvent], ts: i64) -> Option<&str> {
    log.iter()
        .rev()
        .find(|e| e.t <= ts)
        .or_else(|| log.first())
        .map(|e| e.account_id.as_str())
}

pub fn aggregate(entries: &[Entry], account_id: &str, log: &[SwitchEvent]) -> TodayView {
    let mut cost = 0.0;
    let mut turns = 0u32;
    let mut lines = 0u32;
    let mut by_family: HashMap<String, f64> = HashMap::new();
    for e in entries {
        if !log.is_empty() && owner_at(log, e.ts_ms) != Some(account_id) {
            continue;
        }
        cost += e.cost;
        lines += e.lines;
        if e.turn {
            turns += 1;
        }
        if e.cost > 0.0 {
            *by_family.entry(e.family.clone()).or_default() += e.cost;
        }
    }
    let mut models: Vec<(String, f64)> = by_family.into_iter().collect();
    models.sort_by(|a, b| b.1.total_cmp(&a.1));
    TodayView { cost, turns, lines, models }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_and_prices() {
        assert_eq!(family_of("claude-opus-4-5-20251101"), "Opus");
        assert_eq!(family_of("claude-sonnet-5-5"), "Sonnet");
        assert_eq!(price_per_mtok("claude-opus-4-1-20250805"), (15.0, 75.0));
        assert_eq!(price_per_mtok("claude-opus-4-5-20251101"), (5.0, 25.0));
    }

    #[test]
    fn cost_includes_cache_tokens() {
        let usage = serde_json::json!({
            "input_tokens": 1_000_000, "output_tokens": 0,
            "cache_creation_input_tokens": 0, "cache_read_input_tokens": 1_000_000
        });
        // sonnet: $3 for input + $0.30 for cache reads
        assert!((cost_of("claude-sonnet-4-5", &usage) - 3.3).abs() < 1e-9);
    }

    #[test]
    fn attribution_follows_the_switch_log() {
        let log = vec![
            SwitchEvent { t: 100, account_id: "a".into() },
            SwitchEvent { t: 200, account_id: "b".into() },
        ];
        let entries = vec![
            Entry { ts_ms: 150, family: "Opus".into(), cost: 1.0, turn: true, lines: 3 },
            Entry { ts_ms: 250, family: "Opus".into(), cost: 2.0, turn: true, lines: 4 },
        ];
        let a = aggregate(&entries, "a", &log);
        let b = aggregate(&entries, "b", &log);
        assert_eq!((a.cost, a.turns, a.lines), (1.0, 1, 3));
        assert_eq!((b.cost, b.turns, b.lines), (2.0, 1, 4));
    }

    #[test]
    fn line_counting() {
        assert_eq!(count_lines(""), 0);
        assert_eq!(count_lines("a\nb\nc"), 3);
        let write = serde_json::json!({"name": "Write", "input": {"content": "x\ny"}});
        assert_eq!(lines_written(&write), 2);
    }
}
