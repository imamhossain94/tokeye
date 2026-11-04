//! Reads the local usage logs of AI coding tools and turns them into token and cost statistics.
//!
//! Sources: Claude Code, Codex CLI, OpenCode (SQLite and older JSON storage), Gemini CLI,
//! Cline / Roo Code / Kilo Code (in any VS Code-based editor) and Cursor usage CSV exports.

use crate::settings::{PriceRule, Settings, TOOL_KEYS};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

pub fn tool_label(key: &str) -> &'static str {
    match key {
        "claude" => "Claude Code",
        "codex" => "Codex CLI",
        "opencode" => "OpenCode",
        "gemini" => "Gemini CLI",
        "cline" => "Cline",
        "roo" => "Roo Code",
        "kilo" => "Kilo Code",
        "cursor" => "Cursor",
        _ => "Other",
    }
}

pub fn provider_label(key: &str) -> String {
    match key {
        "anthropic" => "Claude.ai / Anthropic".into(),
        "vertex" => "Google Vertex AI".into(),
        "bedrock" => "Amazon Bedrock".into(),
        "openai" => "OpenAI".into(),
        "google" => "Google AI".into(),
        "opencode" => "OpenCode Zen".into(),
        "openrouter" => "OpenRouter".into(),
        "github-copilot" => "GitHub Copilot".into(),
        "cursor" => "Cursor".into(),
        "deepseek" => "DeepSeek".into(),
        "xai" => "xAI".into(),
        other => {
            let mut c = other.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => "Unknown".into(),
            }
        }
    }
}

/// Where each tool keeps its data on this machine.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paths {
    pub claude: PathBuf,
    pub codex: PathBuf,
    pub opencode_db: PathBuf,
    pub opencode_json: PathBuf,
    pub gemini: PathBuf,
    /// (editor name, `<editor>/User/globalStorage`)
    pub vscode_storage: Vec<(String, PathBuf)>,
    pub cursor_csv: PathBuf,
}

struct Rec {
    ts: i64,
    day: NaiveDate,
    hour: usize,
    tool: usize,
    model: usize,
    provider: usize,
    session: usize,
    input: u64,
    cw5: u64,
    cw1: u64,
    cr: u64,
    out: u64,
    web: u64,
    fast: bool,
    /// Cost reported by the tool itself; negative = price it from the table.
    cost: f64,
}

/// One usage event from any source.
struct Ev {
    ts: DateTime<Local>,
    tool: &'static str,
    model: String,
    provider: String,
    session: String,
    project: String,
    /// Full project folder, when the tool records it.
    dir: String,
    input: u64,
    cw5: u64,
    cw1: u64,
    cr: u64,
    out: u64,
    web: u64,
    fast: bool,
    cost: f64,
}

struct SessionInfo {
    key: String,
    tool: usize,
    project: String,
    dir: String,
    first: i64,
    last: i64,
}

#[derive(Default)]
struct Interner {
    names: Vec<String>,
    idx: HashMap<String, usize>,
}

impl Interner {
    fn get(&mut self, s: &str) -> usize {
        if let Some(&i) = self.idx.get(s) {
            return i;
        }
        self.names.push(s.to_string());
        self.idx.insert(s.to_string(), self.names.len() - 1);
        self.names.len() - 1
    }
}

#[derive(Default)]
struct CodexFile {
    session: String,
    cwd: String,
    model: String,
    prev: (u64, u64, u64),
}

// ---------- Claude Code transcript lines ----------

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    cwd: Option<String>,
    message: Option<Msg>,
}

#[derive(Deserialize)]
struct Msg {
    id: Option<String>,
    model: Option<String>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation: Option<CacheCreation>,
    server_tool_use: Option<ServerToolUse>,
    speed: Option<String>,
}

#[derive(Deserialize)]
struct CacheCreation {
    ephemeral_5m_input_tokens: Option<u64>,
    ephemeral_1h_input_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct ServerToolUse {
    web_search_requests: Option<u64>,
}

// ---------- snapshot types sent to the UI ----------

#[derive(Serialize, Default, Clone)]
pub struct Totals {
    pub cost: f64,
    pub input: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    pub output: u64,
    pub requests: u64,
    pub web: u64,
}

impl Totals {
    fn add(&mut self, r: &Rec, cost: f64) {
        self.cost += cost;
        self.input += r.input;
        self.cache_write += r.cw5 + r.cw1;
        self.cache_read += r.cr;
        self.output += r.out;
        self.requests += 1;
        self.web += r.web;
    }
}

#[derive(Serialize, Default, Clone)]
pub struct DayStat {
    pub date: String,
    pub weekday: String,
    #[serde(flatten)]
    pub t: Totals,
}

#[derive(Serialize, Default, Clone)]
pub struct ModelStat {
    pub model: String,
    pub today: Totals,
    pub month: Totals,
}

#[derive(Serialize, Default, Clone)]
pub struct GroupStat {
    pub key: String,
    pub label: String,
    pub today: Totals,
    pub month: Totals,
}

#[derive(Serialize, Default, Clone)]
pub struct SessionStat {
    pub id: String,
    pub tool: String,
    pub project: String,
    pub dir: String,
    pub first: i64,
    pub last: i64,
    /// Claude Code session that only used a Claude.ai / Anthropic account.
    pub personal: bool,
    #[serde(flatten)]
    pub t: Totals,
}

#[derive(Serialize, Default, Clone)]
pub struct SourceStat {
    pub key: String,
    pub label: String,
    pub found: bool,
    pub enabled: bool,
    pub requests: u64,
    pub path: String,
}

#[derive(Serialize, Default, Clone)]
pub struct LimitStat {
    pub used: f64,
    pub limit: f64,
    pub pct: f64,
    pub level: u8,
}

#[derive(Serialize, Default, Clone)]
pub struct Limits {
    pub daily: Option<LimitStat>,
    pub monthly: Option<LimitStat>,
    pub session: Option<LimitStat>,
    pub level: u8,
}

#[derive(Serialize, Default, Clone)]
pub struct Stats {
    pub ready: bool,
    pub now: i64,
    pub last_ingest: i64,
    pub last_ts: i64,
    pub root: String,
    pub root_exists: bool,
    pub today_key: String,
    pub month_key: String,
    pub today: Totals,
    pub yesterday: Totals,
    pub week: Totals,
    pub month: Totals,
    pub all: Totals,
    pub last_hour_cost: f64,
    pub out_last_5m: u64,
    pub projection_month: f64,
    pub days: Vec<DayStat>,
    pub hours: Vec<Totals>,
    pub models: Vec<ModelStat>,
    pub providers: Vec<GroupStat>,
    pub tools: Vec<GroupStat>,
    pub sources: Vec<SourceStat>,
    pub sessions: Vec<SessionStat>,
    pub current: Option<SessionStat>,
    pub limits: Limits,
    pub history_days: i64,
    pub hidden_sessions: usize,
    pub hidden_cost: f64,
    pub clear_before: i64,
}

// ---------- bill explorer ----------

/// Filters for the Bill tab. Empty strings mean "all"; `from`/`to` are unix seconds (0 = open).
#[derive(Deserialize, Default, Clone)]
#[serde(default)]
pub struct BillFilter {
    pub from: i64,
    pub to: i64,
    pub provider: String,
    pub tool: String,
    pub project: String,
    pub session: String,
    pub model: String,
    /// project | session | provider | tool | model | day
    pub group: String,
    pub include_hidden: bool,
}

#[derive(Serialize, Default, Clone)]
pub struct BillRow {
    pub key: String,
    pub label: String,
    pub sub: String,
    /// Project folder for project/session rows (to open in an editor).
    pub dir: String,
    pub share: f64,
    #[serde(flatten)]
    pub t: Totals,
}

#[derive(Serialize, Default, Clone)]
pub struct BillOpt {
    pub key: String,
    pub label: String,
    pub cost: f64,
    pub requests: u64,
}

#[derive(Serialize, Default)]
pub struct BillResult {
    pub total: Totals,
    pub rows: Vec<BillRow>,
    pub days: Vec<DayStat>,
    pub providers: Vec<BillOpt>,
    pub tools: Vec<BillOpt>,
    pub projects: Vec<BillOpt>,
    pub sessions: Vec<BillOpt>,
    pub models: Vec<BillOpt>,
    pub first_ts: i64,
    pub last_ts: i64,
    pub history_days: i64,
}

struct BillCtx<'a> {
    prices: Vec<PriceRule>,
    tool_on: Vec<bool>,
    hidden: HashSet<&'a str>,
}

#[derive(Default)]
struct Group {
    label: String,
    dir: String,
    t: Totals,
    sessions: HashSet<usize>,
    providers: HashSet<usize>,
    tools: HashSet<usize>,
}

fn csv_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn opts<K>(map: HashMap<K, (f64, u64)>, label: impl Fn(&K) -> (String, String)) -> Vec<BillOpt> {
    let mut v: Vec<BillOpt> = map
        .iter()
        .map(|(k, (cost, requests))| {
            let (key, label) = label(k);
            BillOpt { key, label, cost: *cost, requests: *requests }
        })
        .collect();
    v.sort_by(|a, b| b.cost.partial_cmp(&a.cost).unwrap_or(std::cmp::Ordering::Equal));
    v
}

pub struct Engine {
    paths: Paths,
    offsets: HashMap<PathBuf, u64>,
    codex: HashMap<PathBuf, CodexFile>,
    mtimes: HashMap<PathBuf, SystemTime>,
    opencode_seen: i64,
    keys: HashMap<String, usize>,
    recs: Vec<Rec>,
    models: Interner,
    providers: Interner,
    sessions: Vec<SessionInfo>,
    session_idx: HashMap<String, usize>,
    found: Vec<bool>,
    last_ingest: i64,
    ready: bool,
    history_days: i64,
}

// ---------- helpers ----------

fn now_secs() -> i64 {
    Local::now().timestamp()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn project_name(cwd: &str) -> String {
    cwd.rsplit(|c| c == '/' || c == '\\')
        .find(|s| !s.is_empty())
        .unwrap_or(cwd)
        .to_string()
}

fn num(v: &Value, path: &[&str]) -> u64 {
    let mut c = v;
    for k in path {
        c = &c[*k];
    }
    c.as_u64().or_else(|| c.as_f64().map(|f| f.max(0.0) as u64)).unwrap_or(0)
}

fn parse_ts(s: &str) -> Option<DateTime<Local>> {
    if let Ok(t) = DateTime::parse_from_rfc3339(s.trim()) {
        return Some(t.with_timezone(&Local));
    }
    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M", "%m/%d/%Y %H:%M:%S", "%m/%d/%Y %I:%M:%S %p", "%b %d, %Y, %I:%M %p"] {
        if let Ok(n) = NaiveDateTime::parse_from_str(s.trim(), fmt) {
            return Local.from_local_datetime(&n).single();
        }
    }
    None
}

fn split_csv(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

fn parse_number(s: &str) -> Option<f64> {
    let cleaned: String = s.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
    cleaned.parse::<f64>().ok()
}

fn tool_index(key: &str) -> usize {
    TOOL_KEYS.iter().position(|k| *k == key).unwrap_or(0)
}

fn cost_of(r: &Rec, p: &PriceRule, s: &Settings) -> f64 {
    if r.cost >= 0.0 && s.trust_tool_costs {
        return r.cost;
    }
    let tokens = r.input as f64 * p.input
        + r.out as f64 * p.output
        + r.cr as f64 * p.cache_read
        + r.cw5 as f64 * p.cache_write_5m
        + r.cw1 as f64 * p.cache_write_1h;
    let c = tokens / 1_000_000.0 + r.web as f64 * s.web_search_price;
    if r.fast {
        c * s.fast_multiplier
    } else {
        c
    }
}

type OcRow = (String, String, i64, i64, String);

fn opencode_rows(db: &Path, since: i64, cutoff_ms: i64) -> (Vec<(&'static str, OcRow)>, HashMap<String, String>) {
    let mut rows = Vec::new();
    let mut dirs = HashMap::new();
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let Ok(conn) = rusqlite::Connection::open_with_flags(db, flags) else { return (rows, dirs) };
    let _ = conn.busy_timeout(std::time::Duration::from_millis(500));
    for q in ["select id, directory from session_v2", "select id, directory from session"] {
        if let Ok(mut stmt) = conn.prepare(q) {
            if let Ok(it) = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))) {
                for (id, dir) in it.flatten() {
                    if let Some(d) = dir {
                        dirs.entry(id).or_insert(d);
                    }
                }
            }
        }
    }
    let queries = [
        ("message", "select id, session_id, time_created, time_updated, data from message where time_updated > ?1 and time_created >= ?2"),
        ("session_message", "select id, session_id, time_created, time_updated, data from session_message where type = 'assistant' and time_updated > ?1 and time_created >= ?2"),
    ];
    for (table, q) in queries {
        if let Ok(mut stmt) = conn.prepare(q) {
            if let Ok(it) = stmt.query_map(rusqlite::params![since, cutoff_ms], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, String>(4)?))
            }) {
                rows.extend(it.flatten().map(|row| (table, row)));
            }
        }
    }
    (rows, dirs)
}

fn opencode_ev(v: &Value, session: &str, dirs: &HashMap<String, String>, created_ms: i64) -> Option<Ev> {
    if v["role"].as_str().map_or(false, |r| r != "assistant") {
        return None;
    }
    let t = &v["tokens"];
    if !t.is_object() {
        return None;
    }
    let input = num(t, &["input"]);
    let out = num(t, &["output"]) + num(t, &["reasoning"]);
    let cr = num(t, &["cache", "read"]);
    let cw = num(t, &["cache", "write"]);
    if input + out + cr + cw == 0 {
        return None;
    }
    let ms = v["time"]["created"].as_i64().unwrap_or(created_ms);
    let ts = Local.timestamp_millis_opt(ms).single()?;
    let model = v["modelID"].as_str().or(v["model"]["id"].as_str()).unwrap_or("unknown").to_string();
    let provider = v["providerID"].as_str().or(v["model"]["providerID"].as_str()).unwrap_or("opencode").to_string();
    let dir = dirs
        .get(session)
        .cloned()
        .or_else(|| v["path"]["cwd"].as_str().map(String::from))
        .unwrap_or_default();
    Some(Ev {
        ts,
        tool: "opencode",
        model,
        provider,
        session: session.to_string(),
        project: project_name(&dir),
        dir,
        input,
        cw5: cw,
        cw1: 0,
        cr,
        out,
        web: 0,
        fast: false,
        cost: v["cost"].as_f64().unwrap_or(-1.0),
    })
}

impl Engine {
    pub fn new(paths: Paths) -> Self {
        Self {
            paths,
            offsets: HashMap::new(),
            codex: HashMap::new(),
            mtimes: HashMap::new(),
            opencode_seen: 0,
            keys: HashMap::new(),
            recs: Vec::new(),
            models: Interner::default(),
            providers: Interner::default(),
            sessions: Vec::new(),
            session_idx: HashMap::new(),
            found: vec![false; TOOL_KEYS.len()],
            last_ingest: 0,
            ready: false,
            history_days: 62,
        }
    }

    /// Reads anything new from every enabled tool. Returns true when the numbers changed.
    pub fn poll(&mut self, history_days: i64, s: &Settings) -> bool {
        self.history_days = history_days.max(1);
        let before = self.recs.len();
        let mut updated = 0usize;
        let cutoff = (Local::now() - Duration::days(self.history_days)).timestamp();
        let horizon = SystemTime::now() - std::time::Duration::from_secs(self.history_days as u64 * 86_400);
        let p = self.paths.clone();
        let mut found = vec![false; TOOL_KEYS.len()];

        found[tool_index("claude")] = p.claude.is_dir();
        if s.tool_on("claude") && p.claude.is_dir() {
            self.scan_jsonl(&p.claude, "claude", horizon, cutoff, &mut updated);
        }
        found[tool_index("codex")] = p.codex.is_dir();
        if s.tool_on("codex") && p.codex.is_dir() {
            self.scan_jsonl(&p.codex, "codex", horizon, cutoff, &mut updated);
        }
        found[tool_index("opencode")] = p.opencode_db.is_file() || p.opencode_json.is_dir();
        if s.tool_on("opencode") {
            if p.opencode_db.is_file() {
                self.poll_opencode(&p.opencode_db, cutoff, &mut updated);
            }
            if p.opencode_json.is_dir() {
                self.scan_whole(&p.opencode_json, "opencode", "json", horizon, cutoff, &mut updated);
            }
        }
        found[tool_index("gemini")] = p.gemini.is_dir();
        if s.tool_on("gemini") && p.gemini.is_dir() {
            self.scan_whole(&p.gemini, "gemini", "json", horizon, cutoff, &mut updated);
        }
        for (tool, ext_id) in [("cline", "saoudrizwan.claude-dev"), ("roo", "rooveterinaryinc.roo-cline"), ("kilo", "kilocode.kilo-code")] {
            for (_, storage) in &p.vscode_storage {
                let tasks = storage.join(ext_id).join("tasks");
                if tasks.is_dir() {
                    found[tool_index(tool)] = true;
                    if s.tool_on(tool) {
                        self.scan_whole(&tasks, tool, "json", horizon, cutoff, &mut updated);
                    }
                }
            }
        }
        found[tool_index("cursor")] = p.cursor_csv.is_dir();
        if s.tool_on("cursor") && p.cursor_csv.is_dir() {
            self.scan_whole(&p.cursor_csv, "cursor", "csv", horizon, cutoff, &mut updated);
        }
        self.found = found;

        let changed = self.recs.len() != before || updated > 0;
        if changed && self.ready {
            self.last_ingest = now_secs();
        }
        let first = !self.ready;
        self.ready = true;
        changed || first
    }

    fn add(&mut self, key: String, ev: Ev, cutoff: i64, updated: &mut usize) {
        if ev.ts.timestamp() < cutoff {
            return;
        }
        if !key.is_empty() {
            if let Some(&i) = self.keys.get(&key) {
                let r = &mut self.recs[i];
                let before = (r.input, r.out, r.cr, r.cw5, r.cw1, r.web, r.cost.to_bits());
                r.input = r.input.max(ev.input);
                r.out = r.out.max(ev.out);
                r.cr = r.cr.max(ev.cr);
                r.cw5 = r.cw5.max(ev.cw5);
                r.cw1 = r.cw1.max(ev.cw1);
                r.web = r.web.max(ev.web);
                r.cost = r.cost.max(ev.cost);
                if before != (r.input, r.out, r.cr, r.cw5, r.cw1, r.web, r.cost.to_bits()) {
                    *updated += 1;
                }
                return;
            }
        }
        let tool = tool_index(ev.tool);
        let t = ev.ts.timestamp();
        // Claude Code keeps raw session ids (hidden-session lists depend on them).
        let skey = if ev.tool == "claude" { ev.session.clone() } else { format!("{}:{}", ev.tool, ev.session) };
        let session = match self.session_idx.get(&skey) {
            Some(&i) => {
                let si = &mut self.sessions[i];
                si.first = si.first.min(t);
                si.last = si.last.max(t);
                if si.project.is_empty() {
                    si.project = ev.project;
                }
                if si.dir.is_empty() {
                    si.dir = ev.dir;
                }
                i
            }
            None => {
                self.sessions.push(SessionInfo { key: skey.clone(), tool, project: ev.project, dir: ev.dir, first: t, last: t });
                self.session_idx.insert(skey, self.sessions.len() - 1);
                self.sessions.len() - 1
            }
        };
        let rec = Rec {
            ts: t,
            day: ev.ts.date_naive(),
            hour: ev.ts.hour() as usize,
            tool,
            model: self.models.get(&ev.model),
            provider: self.providers.get(&ev.provider),
            session,
            input: ev.input,
            cw5: ev.cw5,
            cw1: ev.cw1,
            cr: ev.cr,
            out: ev.out,
            web: ev.web,
            fast: ev.fast,
            cost: ev.cost,
        };
        if !key.is_empty() {
            self.keys.insert(key, self.recs.len());
        }
        self.recs.push(rec);
    }

    // ----- append-only JSONL logs (Claude Code, Codex) -----

    fn scan_jsonl(&mut self, root: &Path, tool: &'static str, horizon: SystemTime, cutoff: i64, updated: &mut usize) {
        let mut todo: Vec<(PathBuf, u64)> = Vec::new();
        for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().map_or(true, |e| e != "jsonl") {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let len = meta.len();
            match self.offsets.get(path) {
                None => {
                    if meta.modified().map(|m| m < horizon).unwrap_or(false) {
                        continue;
                    }
                    todo.push((path.to_path_buf(), 0));
                }
                Some(&off) if len > off => todo.push((path.to_path_buf(), off)),
                Some(&off) if len < off => todo.push((path.to_path_buf(), 0)),
                _ => {}
            }
        }
        for (path, off) in todo {
            let new_off = self.read_jsonl(&path, off, tool, cutoff, updated);
            self.offsets.insert(path, new_off);
        }
    }

    fn read_jsonl(&mut self, path: &Path, off: u64, tool: &'static str, cutoff: i64, updated: &mut usize) -> u64 {
        let Ok(mut f) = File::open(path) else { return off };
        if f.seek(SeekFrom::Start(off)).is_err() {
            return off;
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            return off;
        }
        let Some(last_nl) = buf.iter().rposition(|&b| b == b'\n') else { return off };
        let mut codex = if tool == "codex" {
            let mut st = self.codex.remove(path).unwrap_or_default();
            if off == 0 {
                st = CodexFile::default();
            }
            Some(st)
        } else {
            None
        };
        let mut pos = 0usize;
        for raw in buf[..=last_nl].split(|&b| b == b'\n') {
            let at = off + pos as u64;
            pos += raw.len() + 1;
            if raw.len() < 32 {
                continue;
            }
            let Ok(line) = std::str::from_utf8(raw) else { continue };
            let line = line.trim_end_matches('\r');
            match codex.as_mut() {
                Some(st) => self.ingest_codex(st, path, line, at, cutoff, updated),
                None => {
                    if contains(raw, b"\"usage\"") && contains(raw, b"\"assistant\"") {
                        self.ingest_claude(line, cutoff, updated);
                    }
                }
            }
        }
        if let Some(st) = codex {
            self.codex.insert(path.to_path_buf(), st);
        }
        off + last_nl as u64 + 1
    }

    fn ingest_claude(&mut self, line: &str, cutoff: i64, updated: &mut usize) {
        let Ok(l) = serde_json::from_str::<Line>(line) else { return };
        if l.kind.as_deref() != Some("assistant") {
            return;
        }
        let Some(msg) = l.message else { return };
        let Some(u) = msg.usage else { return };
        let model = msg.model.unwrap_or_default();
        if model.is_empty() || model.starts_with('<') {
            return;
        }
        let Some(ts) = l.timestamp.as_deref().and_then(parse_ts) else { return };
        let id = msg.id.unwrap_or_default();
        let req = l.request_id.unwrap_or_default();
        let total_cw = u.cache_creation_input_tokens.unwrap_or(0);
        let (mut cw5, mut cw1) = match &u.cache_creation {
            Some(c) => (c.ephemeral_5m_input_tokens.unwrap_or(0), c.ephemeral_1h_input_tokens.unwrap_or(0)),
            None => (total_cw, 0),
        };
        if cw5 + cw1 == 0 && total_cw > 0 {
            cw5 = total_cw;
            cw1 = 0;
        }
        let provider = if id.starts_with("msg_vrtx") || req.starts_with("req_vrtx") {
            "vertex"
        } else if id.starts_with("msg_bdrk") || req.starts_with("req_bdrk") {
            "bedrock"
        } else {
            "anthropic"
        };
        // One response is written as several lines (one per content block); `add` keeps the largest numbers.
        let key = if id.is_empty() && req.is_empty() { String::new() } else { format!("{id}|{req}") };
        let dir = l.cwd.unwrap_or_default();
        let ev = Ev {
            ts,
            tool: "claude",
            model,
            provider: provider.into(),
            session: l.session_id.unwrap_or_default(),
            project: project_name(&dir),
            dir,
            input: u.input_tokens.unwrap_or(0),
            cw5,
            cw1,
            cr: u.cache_read_input_tokens.unwrap_or(0),
            out: u.output_tokens.unwrap_or(0),
            web: u.server_tool_use.as_ref().and_then(|s| s.web_search_requests).unwrap_or(0),
            fast: u.speed.as_deref() == Some("fast"),
            cost: -1.0,
        };
        self.add(key, ev, cutoff, updated);
    }

    fn ingest_codex(&mut self, st: &mut CodexFile, path: &Path, line: &str, at: u64, cutoff: i64, updated: &mut usize) {
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        let p = &v["payload"];
        match v["type"].as_str().unwrap_or("") {
            "session_meta" | "turn_context" => {
                if let Some(id) = p["id"].as_str() {
                    st.session = id.into();
                }
                if let Some(c) = p["cwd"].as_str() {
                    st.cwd = c.into();
                }
                if let Some(m) = p["model"].as_str() {
                    st.model = m.into();
                }
            }
            "event_msg" if p["type"].as_str() == Some("token_count") => {
                let info = &p["info"];
                if !info.is_object() {
                    return;
                }
                let read = |x: &Value| (num(x, &["input_tokens"]), num(x, &["cached_input_tokens"]), num(x, &["output_tokens"]));
                let total = &info["total_token_usage"];
                let (input, cached, out) = if total.is_object() {
                    let cur = read(total);
                    let prev = st.prev;
                    st.prev = cur;
                    if cur.0 >= prev.0 && cur.1 >= prev.1 && cur.2 >= prev.2 {
                        (cur.0 - prev.0, cur.1 - prev.1, cur.2 - prev.2)
                    } else {
                        cur
                    }
                } else if info["last_token_usage"].is_object() {
                    read(&info["last_token_usage"])
                } else {
                    return;
                };
                if input == 0 && out == 0 {
                    return;
                }
                let Some(ts) = v["timestamp"].as_str().and_then(parse_ts) else { return };
                let session = if st.session.is_empty() {
                    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
                } else {
                    st.session.clone()
                };
                let ev = Ev {
                    ts,
                    tool: "codex",
                    model: if st.model.is_empty() { "gpt-5".into() } else { st.model.clone() },
                    provider: "openai".into(),
                    session,
                    project: project_name(&st.cwd),
                    dir: st.cwd.clone(),
                    input: input.saturating_sub(cached),
                    cw5: 0,
                    cw1: 0,
                    cr: cached,
                    out,
                    web: 0,
                    fast: false,
                    cost: -1.0,
                };
                self.add(format!("codex:{}:{at}", path.display()), ev, cutoff, updated);
            }
            _ => {}
        }
    }

    // ----- OpenCode SQLite -----

    fn poll_opencode(&mut self, db: &Path, cutoff: i64, updated: &mut usize) {
        let (rows, dirs) = opencode_rows(db, self.opencode_seen, cutoff * 1000);
        for (table, (id, session, created, upd, data)) in rows {
            self.opencode_seen = self.opencode_seen.max(upd);
            let Ok(v) = serde_json::from_str::<Value>(&data) else { continue };
            if let Some(ev) = opencode_ev(&v, &session, &dirs, created) {
                self.add(format!("oc:{table}:{id}"), ev, cutoff, updated);
            }
        }
    }

    // ----- files rewritten as a whole (Gemini CLI, Cline-family, OpenCode JSON, Cursor CSV) -----

    fn scan_whole(&mut self, root: &Path, tool: &'static str, ext: &str, horizon: SystemTime, cutoff: i64, updated: &mut usize) {
        let mut todo = Vec::new();
        for entry in WalkDir::new(root).max_depth(4).into_iter().filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().map_or(true, |e| e != ext) {
                continue;
            }
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let wanted = match tool {
                "gemini" => path.components().any(|c| c.as_os_str() == "chats"),
                "cline" | "roo" | "kilo" => name == "ui_messages.json",
                _ => true,
            };
            if !wanted {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let Ok(mtime) = meta.modified() else { continue };
            if mtime < horizon || self.mtimes.get(path) == Some(&mtime) {
                continue;
            }
            todo.push((path.to_path_buf(), mtime));
        }
        for (path, mtime) in todo {
            match tool {
                "gemini" => self.parse_gemini(&path, cutoff, updated),
                "cline" | "roo" | "kilo" => self.parse_cline(tool, &path, cutoff, updated),
                "opencode" => self.parse_opencode_json(&path, cutoff, updated),
                "cursor" => self.parse_cursor_csv(&path, cutoff, updated),
                _ => {}
            }
            self.mtimes.insert(path, mtime);
        }
    }

    fn parse_gemini(&mut self, path: &Path, cutoff: i64, updated: &mut usize) {
        let Ok(text) = std::fs::read_to_string(path) else { return };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { return };
        let sid = v["sessionId"].as_str().map(String::from).unwrap_or_else(|| path.display().to_string());
        let project = v["projectHash"]
            .as_str()
            .map(|h| format!("Gemini {}", h.chars().take(6).collect::<String>()))
            .unwrap_or_else(|| "Gemini".into());
        let Some(msgs) = v["messages"].as_array() else { return };
        for (i, m) in msgs.iter().enumerate() {
            let t = &m["tokens"];
            if !t.is_object() {
                continue;
            }
            let input = num(t, &["input"]);
            let cached = num(t, &["cached"]);
            let out = num(t, &["output"]) + num(t, &["thoughts"]);
            if input == 0 && out == 0 {
                continue;
            }
            let Some(ts) = m["timestamp"].as_str().and_then(parse_ts) else { continue };
            let id = m["id"].as_str().map(String::from).unwrap_or_else(|| i.to_string());
            let ev = Ev {
                ts,
                tool: "gemini",
                model: m["model"].as_str().unwrap_or("gemini").to_string(),
                provider: "google".into(),
                session: sid.clone(),
                project: project.clone(),
                dir: String::new(),
                input: input.saturating_sub(cached),
                cw5: 0,
                cw1: 0,
                cr: cached,
                out,
                web: 0,
                fast: false,
                cost: -1.0,
            };
            self.add(format!("gemini:{sid}:{id}"), ev, cutoff, updated);
        }
    }

    fn parse_cline(&mut self, tool: &'static str, path: &Path, cutoff: i64, updated: &mut usize) {
        let Some(task_dir) = path.parent() else { return };
        let task = task_dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let (mut model, mut provider) = (tool.to_string(), tool.to_string());
        if let Ok(meta) = std::fs::read_to_string(task_dir.join("task_metadata.json")) {
            if let Ok(m) = serde_json::from_str::<Value>(&meta) {
                if let Some(last) = m["model_usage"].as_array().and_then(|a| a.last()) {
                    model = last["model_id"].as_str().unwrap_or(&model).to_string();
                    provider = last["model_provider_id"].as_str().unwrap_or(&provider).to_string();
                }
            }
        }
        // .../<Editor>/User/globalStorage/<ext>/tasks/<task>/ui_messages.json
        let editor = path
            .ancestors()
            .nth(6)
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let Ok(text) = std::fs::read_to_string(path) else { return };
        let Ok(Value::Array(msgs)) = serde_json::from_str::<Value>(&text) else { return };
        for m in msgs {
            if m["say"].as_str() != Some("api_req_started") {
                continue;
            }
            let Some(info) = m["text"].as_str().and_then(|t| serde_json::from_str::<Value>(t).ok()) else { continue };
            let (input, out) = (num(&info, &["tokensIn"]), num(&info, &["tokensOut"]));
            let cost = info["cost"].as_f64().unwrap_or(-1.0);
            if input == 0 && out == 0 && cost <= 0.0 {
                continue;
            }
            let ms = m["ts"].as_i64().unwrap_or(0);
            let Some(ts) = Local.timestamp_millis_opt(ms).single() else { continue };
            let ev = Ev {
                ts,
                tool,
                model: model.clone(),
                provider: provider.clone(),
                session: task.clone(),
                project: format!("{} task {}", editor, task.chars().take(6).collect::<String>()),
                dir: String::new(),
                input,
                cw5: num(&info, &["cacheWrites"]),
                cw1: 0,
                cr: num(&info, &["cacheReads"]),
                out,
                web: 0,
                fast: false,
                cost,
            };
            self.add(format!("{tool}:{task}:{ms}"), ev, cutoff, updated);
        }
    }

    fn parse_opencode_json(&mut self, path: &Path, cutoff: i64, updated: &mut usize) {
        let Ok(text) = std::fs::read_to_string(path) else { return };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { return };
        let session = v["sessionID"].as_str().unwrap_or("").to_string();
        let id = v["id"].as_str().map(String::from).unwrap_or_else(|| path.display().to_string());
        if let Some(ev) = opencode_ev(&v, &session, &HashMap::new(), 0) {
            self.add(format!("ocj:{id}"), ev, cutoff, updated);
        }
    }

    /// Cursor's usage export (dashboard → Usage → Export CSV), dropped into the import folder.
    fn parse_cursor_csv(&mut self, path: &Path, cutoff: i64, updated: &mut usize) {
        let Ok(text) = std::fs::read_to_string(path) else { return };
        let mut lines = text.lines();
        let Some(head) = lines.next() else { return };
        let head: Vec<String> = split_csv(head).iter().map(|h| h.trim().to_lowercase()).collect();
        let col = |pred: &dyn Fn(&str) -> bool| head.iter().position(|h| pred(h));
        let date = col(&|h| h.contains("date") || h.contains("time"));
        let model = col(&|h| h.contains("model"));
        let in_with = col(&|h| h.contains("input") && (h.contains("w/ cache") || h.contains("with cache")));
        let in_without = col(&|h| h.contains("input") && (h.contains("w/o") || h.contains("without")));
        let input_plain = col(&|h| h == "input" || h == "input tokens");
        let cache_read = col(&|h| h.contains("cache read"));
        let output = col(&|h| h.contains("output"));
        let cost = col(&|h| h.contains("cost"));
        let Some(date) = date else { return };
        for line in lines {
            let f = split_csv(line);
            let get = |i: Option<usize>| i.and_then(|i| f.get(i)).map(|s| s.trim().to_string()).unwrap_or_default();
            let n = |i: Option<usize>| parse_number(&get(i)).unwrap_or(0.0).max(0.0) as u64;
            let Some(ts) = parse_ts(&get(Some(date))) else { continue };
            let with = n(in_with);
            let without = if in_without.is_some() { n(in_without) } else if input_plain.is_some() { n(input_plain) } else { with };
            let (cr, out) = (n(cache_read), n(output));
            if with + without + cr + out == 0 {
                continue;
            }
            let model_name = get(model);
            let ev = Ev {
                ts,
                tool: "cursor",
                model: if model_name.is_empty() { "cursor".into() } else { model_name.clone() },
                provider: "cursor".into(),
                session: ts.format("%Y-%m-%d").to_string(),
                project: "Cursor".into(),
                dir: String::new(),
                input: without,
                cw5: with.saturating_sub(without),
                cw1: 0,
                cr,
                out,
                web: 0,
                fast: false,
                cost: parse_number(&get(cost)).unwrap_or(-1.0),
            };
            let key = format!("cursor:{}:{}:{}:{}:{}", get(Some(date)), model_name, without, out, cr);
            self.add(key, ev, cutoff, updated);
        }
    }

    // ----- queries -----

    fn project_of(&self, session: usize) -> &str {
        let p = &self.sessions[session].project;
        if p.is_empty() {
            "(no project)"
        } else {
            p
        }
    }

    fn session_label(&self, session: usize) -> String {
        let si = &self.sessions[session];
        let when = Local.timestamp_opt(si.first, 0).single().map(|d| d.format("%b %d %H:%M").to_string()).unwrap_or_default();
        format!("{} · {}", self.project_of(session), when)
    }

    fn bill_ctx<'a>(&self, s: &'a Settings) -> BillCtx<'a> {
        BillCtx {
            prices: self.models.names.iter().map(|m| s.price_for(m)).collect(),
            tool_on: TOOL_KEYS.iter().map(|k| s.tool_on(k)).collect(),
            hidden: s.hidden_sessions.iter().map(|x| x.as_str()).collect(),
        }
    }

    /// Cost of a record and which filters it passes: [provider, tool, project, session, model].
    /// None when it's outside the period, from a disabled tool, or hidden.
    fn bill_check(&self, r: &Rec, s: &Settings, f: &BillFilter, ctx: &BillCtx) -> Option<(f64, [bool; 5])> {
        if !ctx.tool_on[r.tool] || (f.from > 0 && r.ts < f.from) || (f.to > 0 && r.ts >= f.to) {
            return None;
        }
        let si = &self.sessions[r.session];
        if !f.include_hidden && (r.ts < s.clear_before || ctx.hidden.contains(si.key.as_str())) {
            return None;
        }
        let ok = [
            f.provider.is_empty() || f.provider == self.providers.names[r.provider],
            f.tool.is_empty() || f.tool == TOOL_KEYS[r.tool],
            f.project.is_empty() || f.project == self.project_of(r.session),
            f.session.is_empty() || f.session == si.key,
            f.model.is_empty() || f.model == self.models.names[r.model],
        ];
        Some((cost_of(r, &ctx.prices[r.model], s), ok))
    }

    pub fn bill(&self, s: &Settings, f: &BillFilter) -> BillResult {
        let ctx = self.bill_ctx(s);
        let mut res = BillResult { history_days: self.history_days, ..Default::default() };
        let mut fp: HashMap<usize, (f64, u64)> = HashMap::new();
        let mut ft: HashMap<usize, (f64, u64)> = HashMap::new();
        let mut fj: HashMap<String, (f64, u64)> = HashMap::new();
        let mut fs: HashMap<usize, (f64, u64)> = HashMap::new();
        let mut fm: HashMap<usize, (f64, u64)> = HashMap::new();
        let mut groups: HashMap<String, Group> = HashMap::new();
        let mut days: BTreeMap<NaiveDate, Totals> = BTreeMap::new();
        let bump = |m: &mut (f64, u64), c: f64| {
            m.0 += c;
            m.1 += 1;
        };
        for r in &self.recs {
            let Some((c, ok)) = self.bill_check(r, s, f, &ctx) else { continue };
            let all_but = |i: usize| ok.iter().enumerate().all(|(j, v)| j == i || *v);
            // Facets: each list shows what's available given the *other* filters.
            if all_but(0) {
                bump(fp.entry(r.provider).or_default(), c);
            }
            if all_but(1) {
                bump(ft.entry(r.tool).or_default(), c);
            }
            if all_but(2) {
                bump(fj.entry(self.project_of(r.session).to_string()).or_default(), c);
            }
            if all_but(3) {
                bump(fs.entry(r.session).or_default(), c);
            }
            if all_but(4) {
                bump(fm.entry(r.model).or_default(), c);
            }
            if !ok.iter().all(|v| *v) {
                continue;
            }
            res.total.add(r, c);
            days.entry(r.day).or_default().add(r, c);
            res.first_ts = if res.first_ts == 0 { r.ts } else { res.first_ts.min(r.ts) };
            res.last_ts = res.last_ts.max(r.ts);
            let (key, label) = match f.group.as_str() {
                "session" => (self.sessions[r.session].key.clone(), self.session_label(r.session)),
                "provider" => (self.providers.names[r.provider].clone(), provider_label(&self.providers.names[r.provider])),
                "tool" => (TOOL_KEYS[r.tool].to_string(), tool_label(TOOL_KEYS[r.tool]).to_string()),
                "model" => (self.models.names[r.model].clone(), self.models.names[r.model].clone()),
                "day" => (r.day.format("%Y-%m-%d").to_string(), r.day.format("%a, %b %d").to_string()),
                _ => (self.project_of(r.session).to_string(), self.project_of(r.session).to_string()),
            };
            let g = groups.entry(key).or_default();
            if g.label.is_empty() {
                g.label = label;
            }
            if g.dir.is_empty() && matches!(f.group.as_str(), "session" | "project" | "") {
                g.dir = self.sessions[r.session].dir.clone();
            }
            g.t.add(r, c);
            g.sessions.insert(r.session);
            g.providers.insert(r.provider);
            g.tools.insert(r.tool);
        }

        let total_cost = res.total.cost;
        let mut rows: Vec<BillRow> = groups
            .into_iter()
            .map(|(key, g)| {
                let tools: Vec<&str> = g.tools.iter().map(|t| tool_label(TOOL_KEYS[*t])).collect();
                let provs: Vec<String> = g.providers.iter().map(|p| provider_label(&self.providers.names[*p])).collect();
                let sub = match f.group.as_str() {
                    "session" => format!("{} · {}", tools.join(", "), provs.join(", ")),
                    "provider" | "tool" | "model" | "day" => format!("{} session{}", g.sessions.len(), if g.sessions.len() == 1 { "" } else { "s" }),
                    _ => format!("{} session{} · {}", g.sessions.len(), if g.sessions.len() == 1 { "" } else { "s" }, tools.join(", ")),
                };
                BillRow { key, label: g.label, sub, dir: g.dir, share: if total_cost > 0.0 { g.t.cost / total_cost } else { 0.0 }, t: g.t }
            })
            .collect();
        if f.group == "day" {
            rows.sort_by(|a, b| b.key.cmp(&a.key));
        } else {
            rows.sort_by(|a, b| b.t.cost.partial_cmp(&a.t.cost).unwrap_or(std::cmp::Ordering::Equal));
        }
        rows.truncate(300);
        res.rows = rows;

        // Daily series across the period (gaps filled), for the chart.
        let today = Local::now().date_naive();
        let to_day = |ts: i64| Local.timestamp_opt(ts, 0).single().map(|d| d.date_naive());
        let start = if f.from > 0 { to_day(f.from) } else { days.keys().next().copied() };
        let end = if f.to > 0 { to_day(f.to - 1) } else { Some(today) };
        if let (Some(mut d), Some(end)) = (start, end) {
            if (end - d).num_days() > 400 {
                d = end - Duration::days(400);
            }
            while d <= end {
                res.days.push(DayStat {
                    date: d.format("%Y-%m-%d").to_string(),
                    weekday: d.format("%a").to_string(),
                    t: days.get(&d).cloned().unwrap_or_default(),
                });
                d += Duration::days(1);
            }
        }

        res.providers = opts(fp, |k| (self.providers.names[*k].clone(), provider_label(&self.providers.names[*k])));
        res.tools = opts(ft, |k| (TOOL_KEYS[*k].to_string(), tool_label(TOOL_KEYS[*k]).to_string()));
        res.projects = opts(fj, |k| (k.clone(), k.clone()));
        res.sessions = opts(fs, |k| (self.sessions[*k].key.clone(), self.session_label(*k)));
        res.models = opts(fm, |k| (self.models.names[*k].clone(), self.models.names[*k].clone()));
        res
    }

    /// Line items (one per day, tool, provider, session and model) as CSV.
    pub fn bill_csv(&self, s: &Settings, f: &BillFilter) -> String {
        let ctx = self.bill_ctx(s);
        let mut items: BTreeMap<(NaiveDate, usize, usize, usize, usize), (Totals, f64)> = BTreeMap::new();
        let mut total = Totals::default();
        for r in &self.recs {
            let Some((c, ok)) = self.bill_check(r, s, f, &ctx) else { continue };
            if !ok.iter().all(|v| *v) {
                continue;
            }
            let e = items.entry((r.day, r.tool, r.provider, r.session, r.model)).or_default();
            e.0.add(r, c);
            total.add(r, c);
        }
        let mut out = String::from("Date,Tool,Billed to,Project,Session,Model,Requests,Input tokens,Cache write tokens,Cache read tokens,Output tokens,Cost (USD)\n");
        for ((day, tool, prov, sess, model), (t, _)) in items {
            out += &format!(
                "{},{},{},{},{},{},{},{},{},{},{},{:.4}\n",
                day.format("%Y-%m-%d"),
                csv_field(tool_label(TOOL_KEYS[tool])),
                csv_field(&provider_label(&self.providers.names[prov])),
                csv_field(self.project_of(sess)),
                csv_field(&self.sessions[sess].key),
                csv_field(&self.models.names[model]),
                t.requests,
                t.input,
                t.cache_write,
                t.cache_read,
                t.output,
                t.cost
            );
        }
        out += &format!(
            "TOTAL,,,,,,{},{},{},{},{},{:.4}\n",
            total.requests, total.input, total.cache_write, total.cache_read, total.output, total.cost
        );
        out
    }

    /// Claude Code sessions whose requests all went to a Claude.ai / Anthropic account.
    pub fn personal_session_ids(&self) -> Vec<String> {
        let claude = tool_index("claude");
        let anthropic = self.providers.idx.get("anthropic").copied();
        let mut used = vec![false; self.sessions.len()];
        let mut other = vec![false; self.sessions.len()];
        for r in &self.recs {
            used[r.session] = true;
            if r.tool != claude || Some(r.provider) != anthropic {
                other[r.session] = true;
            }
        }
        self.sessions
            .iter()
            .enumerate()
            .filter(|(i, s)| used[*i] && !other[*i] && !s.key.is_empty())
            .map(|(_, s)| s.key.clone())
            .collect()
    }

    pub fn snapshot(&self, s: &Settings) -> Stats {
        let now = Local::now();
        let now_ts = now.timestamp();
        let today = now.date_naive();
        let yesterday = today - Duration::days(1);
        let week_start = today - Duration::days(6);
        let month_start = today.with_day(1).unwrap_or(today);
        let ndays: i64 = 30;
        let first_day = today - Duration::days(ndays - 1);

        let prices: Vec<PriceRule> = self.models.names.iter().map(|m| s.price_for(m)).collect();
        let tool_on: Vec<bool> = TOOL_KEYS.iter().map(|k| s.tool_on(k)).collect();
        let claude = tool_index("claude");
        let anthropic = self.providers.idx.get("anthropic").copied();
        let mut st = Stats {
            ready: self.ready,
            now: now_ts,
            last_ingest: self.last_ingest,
            root: self.paths.claude.to_string_lossy().to_string(),
            root_exists: self.paths.claude.is_dir(),
            today_key: today.format("%Y-%m-%d").to_string(),
            month_key: today.format("%Y-%m").to_string(),
            history_days: self.history_days,
            clear_before: s.clear_before,
            ..Default::default()
        };
        st.days = (0..ndays)
            .map(|i| {
                let d = first_day + Duration::days(i);
                DayStat { date: d.format("%Y-%m-%d").to_string(), weekday: d.format("%a").to_string(), t: Totals::default() }
            })
            .collect();
        st.hours = vec![Totals::default(); 24];
        let mut by_model: HashMap<usize, (Totals, Totals)> = HashMap::new();
        let mut by_prov: HashMap<usize, (Totals, Totals)> = HashMap::new();
        let mut by_tool: HashMap<usize, (Totals, Totals)> = HashMap::new();
        let mut by_session: HashMap<usize, Totals> = HashMap::new();
        let mut personal: HashMap<usize, bool> = HashMap::new();
        let mut per_tool_requests = vec![0u64; TOOL_KEYS.len()];
        let hidden: HashSet<&str> = s.hidden_sessions.iter().map(|x| x.as_str()).collect();
        let mut hidden_ids: HashSet<usize> = HashSet::new();

        for r in &self.recs {
            if !tool_on[r.tool] {
                continue;
            }
            let c = cost_of(r, &prices[r.model], s);
            if r.ts < s.clear_before || hidden.contains(self.sessions[r.session].key.as_str()) {
                st.hidden_cost += c;
                hidden_ids.insert(r.session);
                continue;
            }
            per_tool_requests[r.tool] += 1;
            st.last_ts = st.last_ts.max(r.ts);
            let only_personal = personal.entry(r.session).or_insert(true);
            if r.tool != claude || Some(r.provider) != anthropic {
                *only_personal = false;
            }
            // Provider and tool splits always show everything, so the user can compare.
            for (map, k) in [(&mut by_prov, r.provider), (&mut by_tool, r.tool)] {
                let e = map.entry(k).or_default();
                if r.day == today {
                    e.0.add(r, c);
                }
                if r.day >= month_start {
                    e.1.add(r, c);
                }
            }
            if !s.counts(&self.providers.names[r.provider]) {
                continue;
            }
            st.all.add(r, c);
            by_session.entry(r.session).or_default().add(r, c);
            if r.day == today {
                st.today.add(r, c);
                st.hours[r.hour.min(23)].add(r, c);
            }
            if r.day == yesterday {
                st.yesterday.add(r, c);
            }
            if r.day >= week_start {
                st.week.add(r, c);
            }
            if r.day >= month_start {
                st.month.add(r, c);
            }
            if r.day >= first_day && r.day <= today {
                st.days[(r.day - first_day).num_days() as usize].t.add(r, c);
            }
            let me = by_model.entry(r.model).or_default();
            if r.day == today {
                me.0.add(r, c);
            }
            if r.day >= month_start {
                me.1.add(r, c);
            }
            if now_ts - r.ts <= 3600 {
                st.last_hour_cost += c;
            }
            if now_ts - r.ts <= 300 {
                st.out_last_5m += r.out;
            }
        }

        let by_cost = |a: &GroupStat, b: &GroupStat| b.month.cost.partial_cmp(&a.month.cost).unwrap_or(std::cmp::Ordering::Equal);
        let mut models: Vec<ModelStat> = by_model
            .into_iter()
            .map(|(i, (t, m))| ModelStat { model: self.models.names[i].clone(), today: t, month: m })
            .collect();
        models.sort_by(|a, b| b.month.cost.partial_cmp(&a.month.cost).unwrap_or(std::cmp::Ordering::Equal));
        st.models = models;
        let mut provs: Vec<GroupStat> = by_prov
            .into_iter()
            .map(|(i, (t, m))| {
                let key = self.providers.names[i].clone();
                GroupStat { label: provider_label(&key), key, today: t, month: m }
            })
            .collect();
        provs.sort_by(by_cost);
        st.providers = provs;
        let mut tools: Vec<GroupStat> = by_tool
            .into_iter()
            .map(|(i, (t, m))| GroupStat { key: TOOL_KEYS[i].into(), label: tool_label(TOOL_KEYS[i]).into(), today: t, month: m })
            .collect();
        tools.sort_by(by_cost);
        st.tools = tools;

        let tool_path = |k: &str| -> String {
            let p = &self.paths;
            match k {
                "claude" => p.claude.display().to_string(),
                "codex" => p.codex.display().to_string(),
                "opencode" => p.opencode_db.display().to_string(),
                "gemini" => p.gemini.display().to_string(),
                "cursor" => p.cursor_csv.display().to_string(),
                _ => "VS Code / Cursor / Windsurf extension storage".into(),
            }
        };
        st.sources = TOOL_KEYS
            .iter()
            .enumerate()
            .map(|(i, k)| SourceStat {
                key: k.to_string(),
                label: tool_label(k).into(),
                found: self.found.get(i).copied().unwrap_or(false),
                enabled: tool_on[i],
                requests: per_tool_requests[i],
                path: tool_path(k),
            })
            .collect();

        let mut sessions: Vec<SessionStat> = by_session
            .into_iter()
            .map(|(i, t)| {
                let si = &self.sessions[i];
                SessionStat {
                    id: si.key.clone(),
                    tool: tool_label(TOOL_KEYS[si.tool]).into(),
                    project: si.project.clone(),
                    dir: si.dir.clone(),
                    first: si.first,
                    last: si.last,
                    personal: personal.get(&i).copied().unwrap_or(false),
                    t,
                }
            })
            .collect();
        st.hidden_sessions = hidden_ids.len();
        sessions.sort_by(|a, b| b.last.cmp(&a.last));
        st.current = sessions.first().cloned();
        sessions.truncate(20);
        st.sessions = sessions;

        let elapsed_days = (today.day() as f64 - 1.0) + (now.hour() as f64 + now.minute() as f64 / 60.0) / 24.0;
        let days_in_month = {
            let (y, m) = (today.year(), today.month());
            let next = if m == 12 { NaiveDate::from_ymd_opt(y + 1, 1, 1) } else { NaiveDate::from_ymd_opt(y, m + 1, 1) };
            next.map(|n| (n - month_start).num_days()).unwrap_or(30) as f64
        };
        st.projection_month = if elapsed_days > 0.25 { st.month.cost / elapsed_days * days_in_month } else { st.month.cost };

        let lim = |used: f64, limit: f64| -> Option<LimitStat> {
            if limit <= 0.0 {
                return None;
            }
            let pct = used / limit * 100.0;
            let level = if pct >= 100.0 { 2 } else if pct >= s.warn_percent { 1 } else { 0 };
            Some(LimitStat { used, limit, pct, level })
        };
        let session_cost = st.current.as_ref().map(|c| c.t.cost).unwrap_or(0.0);
        st.limits.daily = lim(st.today.cost, s.daily_limit);
        st.limits.monthly = lim(st.month.cost, s.monthly_limit);
        st.limits.session = lim(session_cost, s.session_limit);
        st.limits.level = [&st.limits.daily, &st.limits.monthly, &st.limits.session]
            .iter()
            .filter_map(|l| l.as_ref().map(|l| l.level))
            .max()
            .unwrap_or(0);
        st
    }
}
