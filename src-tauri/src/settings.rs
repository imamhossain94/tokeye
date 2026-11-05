use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// USD per 1M tokens. The first rule whose `pattern` is contained in the model id wins.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriceRule {
    pub pattern: String,
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write_5m: f64,
    pub cache_write_1h: f64,
}

fn pr(pattern: &str, input: f64, output: f64, cache_read: f64, w5: f64, w1: f64) -> PriceRule {
    PriceRule {
        pattern: pattern.into(),
        input,
        output,
        cache_read,
        cache_write_5m: w5,
        cache_write_1h: w1,
    }
}

pub fn default_prices() -> Vec<PriceRule> {
    vec![
        // Free tiers (e.g. OpenCode "-free" models)
        pr("-free", 0.0, 0.0, 0.0, 0.0, 0.0),
        // Anthropic
        pr("opus-5-5", 4.0, 20.0, 0.20, 5.0, 8.0),
        pr("sonnet-5-5", 2.0, 10.0, 0.20, 2.5, 4.0),
        pr("haiku-5-5", 0.10, 0.50, 0.01, 0.125, 0.20),
        pr("opus-5", 5.0, 25.0, 0.50, 6.25, 10.0),
        pr("fable", 5.0, 25.0, 0.50, 6.25, 10.0),
        pr("sonnet-5", 3.0, 15.0, 0.30, 3.75, 6.0),
        pr("opus-4-1", 15.0, 75.0, 1.50, 18.75, 30.0),
        pr("opus-4-2025", 15.0, 75.0, 1.50, 18.75, 30.0),
        pr("opus-4", 5.0, 25.0, 0.50, 6.25, 10.0),
        pr("sonnet", 3.0, 15.0, 0.30, 3.75, 6.0),
        pr("haiku-4", 1.0, 5.0, 0.10, 1.25, 2.0),
        pr("haiku", 0.80, 4.0, 0.08, 1.0, 1.6),
        pr("opus", 4.0, 20.0, 0.20, 5.0, 8.0),
        // OpenAI
        pr("gpt-5-mini", 0.25, 2.0, 0.025, 0.25, 0.25),
        pr("gpt-5-nano", 0.05, 0.40, 0.005, 0.05, 0.05),
        pr("gpt-5", 1.25, 10.0, 0.125, 1.25, 1.25),
        pr("gpt-4.1-mini", 0.40, 1.60, 0.10, 0.40, 0.40),
        pr("gpt-4.1", 2.0, 8.0, 0.50, 2.0, 2.0),
        pr("gpt-4o-mini", 0.15, 0.60, 0.075, 0.15, 0.15),
        pr("gpt-4o", 2.5, 10.0, 1.25, 2.5, 2.5),
        pr("o4-mini", 1.10, 4.40, 0.275, 1.10, 1.10),
        pr("o3", 2.0, 8.0, 0.50, 2.0, 2.0),
        pr("codex-mini", 1.50, 6.0, 0.375, 1.50, 1.50),
        // Google
        pr("gemini-2.5-flash-lite", 0.10, 0.40, 0.025, 0.10, 0.10),
        pr("gemini-2.5-flash", 0.30, 2.50, 0.075, 0.30, 0.30),
        pr("gemini-2.5-pro", 1.25, 10.0, 0.31, 1.25, 1.25),
        pr("gemini-3", 2.0, 12.0, 0.20, 2.0, 2.0),
        pr("gemini", 0.30, 2.50, 0.075, 0.30, 0.30),
        // Others
        pr("deepseek", 0.28, 0.42, 0.028, 0.28, 0.28),
        pr("grok-code", 0.20, 1.50, 0.02, 0.20, 0.20),
        pr("grok", 3.0, 15.0, 0.75, 3.0, 3.0),
        pr("kimi", 0.60, 2.50, 0.15, 0.60, 0.60),
        pr("glm", 0.60, 2.20, 0.11, 0.60, 0.60),
        pr("qwen", 0.40, 1.60, 0.08, 0.40, 0.40),
        pr("minimax", 0.30, 1.20, 0.03, 0.30, 0.30),
        // Anything else
        pr("", 4.0, 20.0, 0.20, 5.0, 8.0),
    ]
}

/// How one on-screen surface (notch, bubble, widget, taskbar strip) looks and behaves.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Surface {
    /// Size multiplier (0.6 – 2.0).
    pub scale: f64,
    pub opacity: f64,
    /// The big number: today | month | total | session | tokens_in | tokens_out | rate | requests
    pub metric: String,
    /// What the numbers count: "" (everything), "live" (the latest session),
    /// "project:<name>" or "session:<key>".
    pub scope: String,
    /// Extra items: in, out, month, today, session, requests, limit, chart, live
    pub fields: Vec<String>,
    /// solid | glass | outline | minimal
    pub style: String,
    /// Corner radius in px.
    pub radius: f64,
    /// Bubble/widget: stay above other windows.
    pub on_top: bool,
    /// Bubble/widget: let clicks pass through (unlock from the tray menu).
    pub ghost: bool,
    /// Notch: left | center | right | custom (dragged; the spot is in `Settings::positions`)
    pub anchor: String,
    /// Notch: expands at all.
    pub expand: bool,
    /// Notch: hover | click
    pub expand_on: String,
    /// Hide after this many idle seconds (0 = never). Touching the top screen edge shows it again.
    pub auto_hide: u32,
    /// Notch: what brings it back when hidden: edge (anywhere on the top edge) | notch (only above it)
    pub reveal: String,
    /// Widget chart range in days.
    pub chart_days: u32,
}

impl Default for Surface {
    fn default() -> Self {
        Self {
            scale: 1.0,
            opacity: 1.0,
            metric: "today".into(),
            scope: String::new(),
            fields: vec!["in".into(), "out".into(), "month".into(), "limit".into(), "live".into()],
            style: "solid".into(),
            radius: 8.0,
            on_top: true,
            ghost: false,
            anchor: "center".into(),
            expand: true,
            expand_on: "hover".into(),
            auto_hide: 0,
            reveal: "edge".into(),
            chart_days: 14,
        }
    }
}

fn surface(fields: &[&str], on_top: bool) -> Surface {
    Surface {
        fields: fields.iter().map(|f| f.to_string()).collect(),
        on_top,
        ..Surface::default()
    }
}

pub fn default_surfaces() -> HashMap<String, Surface> {
    HashMap::from([
        ("notch".to_string(), surface(&["in", "out", "month", "session", "limit", "chart", "live"], true)),
        ("floating".to_string(), surface(&["in", "out", "month", "limit", "live"], true)),
        ("widget".to_string(), surface(&["month", "in", "out", "requests", "limit", "chart", "live"], false)),
        ("taskbar".to_string(), surface(&["in", "out", "limit", "live"], true)),
    ])
}

pub const TOOL_KEYS: [&str; 8] = ["claude", "codex", "opencode", "gemini", "cline", "roo", "kilo", "cursor"];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // Limits (0 = off) and alerts
    pub daily_limit: f64,
    pub monthly_limit: f64,
    pub session_limit: f64,
    pub warn_percent: f64,
    pub notify: bool,
    // Surfaces
    pub show_notch: bool,
    pub show_floating: bool,
    pub show_widget: bool,
    pub show_taskbar: bool,
    pub surfaces: HashMap<String, Surface>,
    // Companion
    pub companion: bool,
    pub eye_shape: String,
    pub follow_cursor: bool,
    pub personality: String,
    // Look
    pub theme: String,
    pub accent: String,
    pub eye_color: String,
    pub opacity: f64,
    // Data sources
    pub tools: HashMap<String, bool>,
    pub tool_paths: HashMap<String, String>,
    pub trust_tool_costs: bool,
    pub providers: String,
    pub claude_dir: String,
    pub poll_ms: u64,
    pub history_days: u32,
    pub prices: Vec<PriceRule>,
    pub web_search_price: f64,
    pub fast_multiplier: f64,
    pub currency: String,
    /// Command that opens a project folder: "code", "cursor", "windsurf", ... or anything with {path}.
    pub editor: String,
    // System
    pub autostart: bool,
    pub first_run_done: bool,
    pub positions: HashMap<String, (i32, i32)>,
    // Cleared data: hidden session ids, and "ignore everything before" (unix secs, 0 = off)
    pub hidden_sessions: Vec<String>,
    pub clear_before: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            daily_limit: 100.0,
            monthly_limit: 2500.0,
            session_limit: 0.0,
            warn_percent: 80.0,
            notify: true,
            show_notch: true,
            show_floating: false,
            show_widget: false,
            show_taskbar: cfg!(target_os = "windows"),
            surfaces: default_surfaces(),
            companion: true,
            eye_shape: "rounded".into(),
            follow_cursor: true,
            personality: "normal".into(),
            theme: "system".into(),
            accent: "#D97757".into(),
            eye_color: "#9AF7FF".into(),
            opacity: 1.0,
            tools: TOOL_KEYS.iter().map(|k| (k.to_string(), true)).collect(),
            tool_paths: HashMap::new(),
            trust_tool_costs: true,
            providers: "all".into(),
            claude_dir: String::new(),
            poll_ms: 1500,
            history_days: 62,
            prices: default_prices(),
            web_search_price: 0.01,
            fast_multiplier: 2.0,
            currency: "$".into(),
            editor: "code".into(),
            autostart: true,
            first_run_done: false,
            positions: HashMap::new(),
            hidden_sessions: Vec::new(),
            clear_before: 0,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        let mut s: Settings = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        s.migrate();
        s
    }

    /// Fills in anything added in newer versions (price rules, surfaces, tools).
    fn migrate(&mut self) {
        let known: Vec<String> = self.prices.iter().map(|p| p.pattern.clone()).collect();
        for rule in default_prices() {
            if !known.contains(&rule.pattern) {
                let at = self.prices.iter().position(|p| p.pattern.is_empty()).unwrap_or(self.prices.len());
                // "-free" must win over every other pattern.
                let at = if rule.pattern == "-free" { 0 } else { at };
                self.prices.insert(at, rule);
            }
        }
        for (k, v) in default_surfaces() {
            self.surfaces.entry(k).or_insert(v);
        }
        for k in TOOL_KEYS {
            self.tools.entry(k.to_string()).or_insert(true);
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self).unwrap_or_default())
    }

    pub fn surface(&self, label: &str) -> Surface {
        self.surfaces
            .get(label)
            .cloned()
            .or_else(|| default_surfaces().remove(label))
            .unwrap_or_default()
    }

    pub fn tool_on(&self, key: &str) -> bool {
        *self.tools.get(key).unwrap_or(&true)
    }

    pub fn price_for(&self, model: &str) -> PriceRule {
        let m = model.to_lowercase();
        let rules = if self.prices.is_empty() { default_prices() } else { self.prices.clone() };
        rules
            .iter()
            .find(|r| m.contains(&r.pattern.to_lowercase()))
            .cloned()
            .unwrap_or_else(|| default_prices().pop().unwrap())
    }

    pub fn counts(&self, provider: &str) -> bool {
        self.providers == "all" || self.providers.is_empty() || self.providers == provider
    }
}
