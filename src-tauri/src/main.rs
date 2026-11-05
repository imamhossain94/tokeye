#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod engine;
mod settings;

use engine::{Engine, Paths, Stats};
use settings::{Settings, Surface};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Position, RunEvent, Size, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_notification::NotificationExt;

const OVERLAYS: [&str; 4] = ["notch", "floating", "widget", "taskbar"];
const TRAY_OK: &[u8] = include_bytes!("../icons/tray.png");
const TRAY_WARN: &[u8] = include_bytes!("../icons/tray-warn.png");
const TRAY_OVER: &[u8] = include_bytes!("../icons/tray-over.png");

struct AppState {
    settings: Mutex<Settings>,
    stats: Mutex<serde_json::Value>,
    engine: Mutex<Engine>,
    config_path: PathBuf,
    alerted: Mutex<HashMap<String, u8>>,
    hidden_at: Mutex<HashMap<String, Instant>>,
    tray_level: Mutex<u8>,
    new_paths: Mutex<Option<Paths>>,
    pinned: AtomicBool,
    flyout: AtomicBool,
    dirty: AtomicBool,
    save_pending: AtomicBool,
    dragging: AtomicBool,
}

fn st(app: &AppHandle) -> &AppState {
    app.state::<AppState>().inner()
}

fn claude_root(app: &AppHandle, s: &Settings) -> PathBuf {
    if !s.claude_dir.trim().is_empty() {
        return PathBuf::from(s.claude_dir.trim()).join("projects");
    }
    if let Ok(d) = std::env::var("CLAUDE_CONFIG_DIR") {
        if !d.trim().is_empty() {
            return PathBuf::from(d.trim()).join("projects");
        }
    }
    app.path().home_dir().unwrap_or_default().join(".claude").join("projects")
}

/// Where every supported tool keeps its usage data (overridable per tool in settings).
fn resolve_paths(app: &AppHandle, s: &Settings) -> Paths {
    let home = app.path().home_dir().unwrap_or_default();
    let custom = |k: &str| s.tool_paths.get(k).map(|p| p.trim()).filter(|p| !p.is_empty()).map(PathBuf::from);
    let env_dir = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty()).map(PathBuf::from);
    let codex = custom("codex").unwrap_or_else(|| env_dir("CODEX_HOME").unwrap_or_else(|| home.join(".codex")).join("sessions"));
    let opencode = custom("opencode").unwrap_or_else(|| {
        env_dir("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local").join("share")).join("opencode")
    });
    let config = app.path().config_dir().unwrap_or_default();
    let vscode_storage = ["Code", "Code - Insiders", "Cursor", "Windsurf", "VSCodium", "Trae", "Kiro", "Void"]
        .iter()
        .map(|e| (e.to_string(), config.join(e).join("User").join("globalStorage")))
        .filter(|(_, p)| p.is_dir())
        .collect();
    Paths {
        claude: claude_root(app, s),
        codex,
        opencode_db: opencode.join("opencode.db"),
        opencode_json: opencode.join("storage").join("message"),
        gemini: custom("gemini").unwrap_or_else(|| home.join(".gemini").join("tmp")),
        vscode_storage,
        cursor_csv: custom("cursor").unwrap_or_else(|| home.join("Tokeye").join("imports")),
    }
}

/// Puts a window at the top of the always-on-top band (the taskbar raises itself when clicked).
fn raise(w: &WebviewWindow) {
    #[cfg(windows)]
    {
        if let Ok(h) = w.hwnd() {
            win32::raise_topmost(h.0 as isize);
            return;
        }
    }
    let _ = w.set_always_on_top(true);
}

#[cfg(windows)]
mod win32 {
    #[link(name = "user32")]
    extern "system" {
        fn SetWindowPos(hwnd: isize, after: isize, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
        fn GetAsyncKeyState(key: i32) -> i16;
    }
    /// Whether a mouse button is held (physical buttons, so swapped buttons work too).
    pub fn mouse_down() -> bool {
        unsafe { GetAsyncKeyState(0x01) < 0 || GetAsyncKeyState(0x02) < 0 }
    }
    pub fn raise_topmost(hwnd: isize) {
        // HWND_TOPMOST; SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE | SWP_NOOWNERZORDER
        unsafe {
            SetWindowPos(hwnd, -1, 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010 | 0x0200);
        }
    }
}

fn fmt_tok(n: u64) -> String {
    let f = n as f64;
    if f >= 1e9 {
        format!("{:.2}B", f / 1e9)
    } else if f >= 1e6 {
        format!("{:.1}M", f / 1e6)
    } else if f >= 1e3 {
        format!("{:.1}k", f / 1e3)
    } else {
        n.to_string()
    }
}

// ---------------- windows ----------------

fn hide(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.hide();
    }
    st(app).hidden_at.lock().unwrap().insert(label.to_string(), Instant::now());
}

fn recently_hidden(app: &AppHandle, label: &str, ms: u64) -> bool {
    st(app)
        .hidden_at
        .lock()
        .unwrap()
        .get(label)
        .map_or(false, |t| t.elapsed() < Duration::from_millis(ms))
}

enum Placement {
    Flyout,
    Menu,
    Center,
}

fn place(app: &AppHandle, w: &WebviewWindow, c: PhysicalPosition<f64>, kind: Placement) {
    let Ok(size) = w.outer_size() else { return };
    let (pw, ph) = (size.width as i32, size.height as i32);
    let mon = app
        .monitor_from_point(c.x, c.y)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(mon) = mon else { return };
    let wa = mon.work_area();
    let (l, t) = (wa.position.x, wa.position.y);
    let (r, b) = (l + wa.size.width as i32, t + wa.size.height as i32);
    let m = (10.0 * mon.scale_factor()) as i32;
    let (cx, cy) = (c.x as i32, c.y as i32);
    let (x, y) = match kind {
        Placement::Flyout => (cx - pw / 2, if cy > (t + b) / 2 { b - ph - m } else { t + m }),
        Placement::Menu => (
            if cx + pw > r - m { cx - pw } else { cx },
            if cy + ph > b - m { cy - ph } else { cy },
        ),
        Placement::Center => ((l + r - pw) / 2, (t + b - ph) / 2),
    };
    let x = x.clamp(l + m, (r - pw - m).max(l + m));
    let y = y.clamp(t + m, (b - ph - m).max(t + m));
    let _ = w.set_position(Position::Physical(PhysicalPosition::new(x, y)));
}

fn show_dashboard(app: &AppHandle, anchor: Option<PhysicalPosition<f64>>, tab: Option<&str>) {
    let Some(w) = app.get_webview_window("dashboard") else { return };
    if !w.is_visible().unwrap_or(false) {
        // From the tray it behaves like a flyout (hides when you click away); otherwise it's a normal window.
        st(app).flyout.store(anchor.is_some(), Ordering::SeqCst);
        let kind = if anchor.is_some() { Placement::Flyout } else { Placement::Center };
        if let Some(c) = anchor.or_else(|| app.cursor_position().ok()) {
            place(app, &w, c, kind);
        }
    }
    let _ = w.show();
    let _ = w.unminimize();
    let _ = w.set_focus();
    if let Some(t) = tab {
        let _ = app.emit_to("dashboard", "goto", t);
    }
}

fn toggle_dashboard(app: &AppHandle, pos: PhysicalPosition<f64>) {
    let Some(w) = app.get_webview_window("dashboard") else { return };
    if w.is_visible().unwrap_or(false) {
        hide(app, "dashboard");
    } else if !recently_hidden(app, "dashboard", 350) {
        show_dashboard(app, Some(pos), None);
    }
}

/// Opens the custom menu. `from` is the surface it was opened from (it can then change what that surface shows).
fn open_menu(app: &AppHandle, anchor: Option<PhysicalPosition<f64>>, from: Option<&str>) {
    let Some(w) = app.get_webview_window("menu") else { return };
    if w.is_visible().unwrap_or(false) {
        hide(app, "menu");
        return;
    }
    if recently_hidden(app, "menu", 250) {
        return;
    }
    if let Some(c) = anchor.or_else(|| app.cursor_position().ok()) {
        place(app, &w, c, Placement::Menu);
    }
    let _ = app.emit_to("menu", "menu-open", from);
    let _ = w.show();
    let _ = w.set_focus();
    // On Linux the first focus request races the window's first draw (tao only
    // restores accept-focus after that), so ask again once it is really mapped.
    #[cfg(target_os = "linux")]
    {
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(80));
            if let Some(w) = app.get_webview_window("menu") {
                if w.is_visible().unwrap_or(false) {
                    let _ = w.set_focus();
                }
            }
        });
    }
}

fn create_dashboard(app: &AppHandle) -> tauri::Result<()> {
    let w = WebviewWindowBuilder::new(app, "dashboard", WebviewUrl::App("index.html".into()))
        .title("Tokeye")
        .inner_size(440.0, 700.0)
        .min_inner_size(380.0, 520.0)
        .decorations(false)
        .shadow(true)
        .resizable(true)
        .visible(false)
        .build()?;
    let app2 = app.clone();
    w.on_window_event(move |e| match e {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide(&app2, "dashboard");
        }
        WindowEvent::Focused(false) => {
            let s = st(&app2);
            if s.flyout.load(Ordering::SeqCst) && !s.pinned.load(Ordering::SeqCst) {
                hide(&app2, "dashboard");
            }
        }
        _ => {}
    });
    Ok(())
}

fn create_menu(app: &AppHandle) -> tauri::Result<()> {
    let w = WebviewWindowBuilder::new(app, "menu", WebviewUrl::App("menu.html".into()))
        .title("Tokeye menu")
        .inner_size(280.0, 452.0)
        .decorations(false)
        .shadow(true)
        // GTK won't programmatically resize a fixed-size window (the menu fits itself to
        // its content); on Windows and macOS a resizable window would get resize borders.
        .resizable(cfg!(target_os = "linux"))
        .maximizable(false)
        .minimizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .visible(false)
        .build()?;
    let app2 = app.clone();
    w.on_window_event(move |e| match e {
        WindowEvent::Focused(false) => hide(&app2, "menu"),
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide(&app2, "menu");
        }
        _ => {}
    });
    Ok(())
}

fn overlay_size(label: &str) -> (f64, f64) {
    match label {
        "notch" => (520.0, 240.0),
        "floating" => (300.0, 112.0),
        "widget" => (340.0, 252.0),
        _ => (290.0, 40.0),
    }
}

fn overlay_enabled(s: &Settings, label: &str) -> bool {
    match label {
        "notch" => s.show_notch,
        "floating" => s.show_floating,
        "widget" => s.show_widget,
        "taskbar" => s.show_taskbar,
        _ => false,
    }
}

fn scaled_size(label: &str, cfg: &Surface) -> (f64, f64) {
    let (w, h) = overlay_size(label);
    let k = cfg.scale.clamp(0.6, 2.0);
    (w * k, h * k)
}

/// Linux: limits which parts of a window take mouse input (rects in CSS px: x, y, width,
/// height). The notch window stays big enough for the open panel and its shadow while the
/// transparent rest passes clicks through, and XWayland delivers real pointer events
/// inside the region, which the global cursor poll can't see under Wayland.
#[allow(unused_variables)]
fn set_input_region(app: &AppHandle, w: &WebviewWindow, rects: Vec<[f64; 4]>) {
    #[cfg(target_os = "linux")]
    {
        let w = w.clone();
        // GTK must only be touched from the main thread (this is also called from async commands).
        let _ = app.run_on_main_thread(move || {
            use gtk::prelude::*;
            let Ok(win) = w.gtk_window() else { return };
            let region = cairo::Region::create();
            for [x, y, rw, rh] in rects {
                let r = cairo::RectangleInt::new(x.floor() as i32, y.floor() as i32, rw.ceil().max(1.0) as i32, rh.ceil().max(1.0) as i32);
                let _ = region.union_rectangle(&r);
            }
            win.input_shape_combine_region(Some(&region));
        });
    }
}

fn default_overlay_pos(app: &AppHandle, label: &str, cfg: &Surface) -> Option<(i32, i32)> {
    // X11 without a primary output (common under XWayland) reports none; use the first screen then.
    let mon = app
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| app.available_monitors().ok().and_then(|m| m.into_iter().next()))?;
    let sf = mon.scale_factor();
    let px = |v: f64| (v * sf) as i32;
    let (lw, lh) = scaled_size(label, cfg);
    let (pw, ph) = (px(lw), px(lh));
    let (mx, my) = (mon.position().x, mon.position().y);
    let (mw, mh) = (mon.size().width as i32, mon.size().height as i32);
    let wa = mon.work_area();
    Some(match label {
        "notch" => match cfg.anchor.as_str() {
            "left" => (mx + px(24.0), my),
            "right" => (mx + mw - pw - px(24.0), my),
            _ => (mx + (mw - pw) / 2, my),
        },
        "floating" => (mx + mw - pw - px(24.0), wa.position.y + px(24.0)),
        "widget" => (wa.position.x + px(32.0), wa.position.y + px(32.0)),
        _ => {
            let bar = (mh - wa.size.height as i32).max(px(40.0));
            let bar_top = if wa.position.y > my { my } else { my + mh - bar };
            (mx + mw - pw - px(380.0), bar_top + (bar - ph) / 2)
        }
    })
}

fn create_overlay(app: &AppHandle, label: &str, s: &Settings) -> tauri::Result<()> {
    let cfg = s.surface(label);
    let (lw, lh) = scaled_size(label, &cfg);
    let on_top = matches!(label, "notch" | "taskbar") || cfg.on_top;
    let w = WebviewWindowBuilder::new(app, label, WebviewUrl::App("overlay.html".into()))
        .title(format!("Tokeye {label}"))
        .inner_size(lw, lh)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(cfg!(target_os = "linux"))
        .maximizable(false)
        .minimizable(false)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .always_on_top(on_top)
        .always_on_bottom(label == "widget" && !on_top)
        .visible_on_all_workspaces(label == "notch")
        .build()?;
    // The notch sits at its anchor unless it was dragged somewhere.
    let pos = if label == "notch" && cfg.anchor != "custom" { None } else { s.positions.get(label).copied() };
    if let Some((x, y)) = pos.or_else(|| default_overlay_pos(app, label, &cfg)) {
        let _ = w.set_position(Position::Physical(PhysicalPosition::new(x, y)));
    }
    let _ = w.show();
    // Click-through must be set after the window is realized. On Linux tao's
    // `CursorIgnoreEvents` handler unwraps the GDK window, which is still `None`
    // before `show()`, so doing this first aborts the whole app on startup.
    // Until the page reports its own shape, only the closed pill takes input.
    if cfg!(target_os = "linux") && label == "notch" {
        let k = cfg.scale.clamp(0.6, 2.0);
        set_input_region(app, &w, vec![[(lw - 204.0 * k) / 2.0, 0.0, 204.0 * k, 36.0 * k]]);
    }
    let notch_click_through = label == "notch" && !cfg!(target_os = "linux");
    if notch_click_through || (cfg.ghost && matches!(label, "floating" | "widget")) {
        let _ = w.set_ignore_cursor_events(true);
    }
    // Changing click-through drops the topmost flag on Windows, so set it again.
    if on_top {
        raise(&w);
    }
    if label != "notch" {
        let app2 = app.clone();
        let lbl = label.to_string();
        w.on_window_event(move |e| {
            if let WindowEvent::Moved(p) = e {
                let state = st(&app2);
                state.settings.lock().unwrap().positions.insert(lbl.clone(), (p.x, p.y));
                state.save_pending.store(true, Ordering::SeqCst);
            }
        });
    }
    Ok(())
}

fn apply_overlays(app: &AppHandle) {
    let s = st(app).settings.lock().unwrap().clone();
    for label in OVERLAYS {
        match (overlay_enabled(&s, label), app.get_webview_window(label)) {
            (true, None) => {
                let _ = create_overlay(app, label, &s);
            }
            (false, Some(w)) => {
                let _ = w.destroy();
            }
            _ => {}
        }
    }
}

/// Applies size, position and window flags from the surface settings to open overlays.
fn apply_surfaces(app: &AppHandle, s: &Settings) {
    for label in OVERLAYS {
        let Some(w) = app.get_webview_window(label) else { continue };
        let cfg = s.surface(label);
        let (lw, lh) = scaled_size(label, &cfg);
        let old_size = w.outer_size().ok();
        let _ = w.set_size(Size::Logical(LogicalSize::new(lw, lh)));
        match label {
            "notch" => {
                if cfg.anchor == "custom" {
                    // A dragged notch stays put; keep the pill centered on the same spot if it was resized.
                    if let (Ok(p), Some(old)) = (w.outer_position(), old_size) {
                        let new_w = (lw * w.scale_factor().unwrap_or(1.0)).round() as i32;
                        let x = p.x + (old.width as i32 - new_w) / 2;
                        if x != p.x {
                            let _ = w.set_position(Position::Physical(PhysicalPosition::new(x, p.y)));
                            let state = st(app);
                            state.settings.lock().unwrap().positions.insert("notch".into(), (x, p.y));
                            state.save_pending.store(true, Ordering::SeqCst);
                        }
                    }
                } else if let Some((x, y)) = default_overlay_pos(app, label, &cfg) {
                    let _ = w.set_position(Position::Physical(PhysicalPosition::new(x, y)));
                }
                raise(&w);
            }
            "floating" | "widget" => {
                let _ = w.set_ignore_cursor_events(cfg.ghost);
                if cfg.on_top {
                    let _ = w.set_always_on_bottom(false);
                    let _ = w.set_always_on_top(true);
                    raise(&w);
                } else {
                    let _ = w.set_always_on_top(false);
                    let _ = w.set_always_on_bottom(label == "widget");
                }
            }
            _ => raise(&w),
        }
    }
}

fn sync_autostart(app: &AppHandle, enabled: bool) {
    let al = app.autolaunch();
    let current = al.is_enabled().unwrap_or(false);
    if enabled && !current {
        let _ = al.enable();
    } else if !enabled && current {
        let _ = al.disable();
    }
}

// ---------------- tray ----------------

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    #[allow(unused_mut)]
    let mut builder = TrayIconBuilder::with_id("main")
        .icon(Image::from_bytes(TRAY_OK)?)
        .tooltip("Tokeye — reading Claude Code history…")
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button, button_state: MouseButtonState::Up, position, .. } = event {
                let app = tray.app_handle();
                match button {
                    MouseButton::Left => toggle_dashboard(app, position),
                    MouseButton::Right => open_menu(app, Some(position), None),
                    _ => {}
                }
            }
        });
    // Linux tray icons can't report clicks, so they get a native menu instead.
    #[cfg(target_os = "linux")]
    {
        use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
        let open = MenuItem::with_id(app, "open", "Open dashboard", true, None::<&str>)?;
        let quick = MenuItem::with_id(app, "menu", "Quick menu…", true, None::<&str>)?;
        let quit = MenuItem::with_id(app, "quit", "Quit Tokeye", true, None::<&str>)?;
        let sep = PredefinedMenuItem::separator(app)?;
        let menu = Menu::with_items(app, &[&open, &quick, &sep, &quit])?;
        builder = builder.menu(&menu).on_menu_event(|app, e| match e.id().as_ref() {
            "open" => show_dashboard(app, None, None),
            "menu" => open_menu(app, None, None),
            "quit" => app.exit(0),
            _ => {}
        });
    }
    builder.build(app)?;
    Ok(())
}

fn update_tray(app: &AppHandle, s: &Stats, cfg: &Settings) {
    let Some(tray) = app.tray_by_id("main") else { return };
    let c = &cfg.currency;
    let tip = format!(
        "Tokeye · Today {c}{:.2} · Month {c}{:.2}\nIn {} · Out {} · {} requests",
        s.today.cost,
        s.month.cost,
        fmt_tok(s.today.input + s.today.cache_write + s.today.cache_read),
        fmt_tok(s.today.output),
        s.today.requests
    );
    let _ = tray.set_tooltip(Some(&tip));
    #[cfg(target_os = "macos")]
    let _ = tray.set_title(Some(&format!("{c}{:.2}", s.today.cost)));
    let level = s.limits.level;
    let mut last = st(app).tray_level.lock().unwrap();
    if *last != level {
        *last = level;
        let bytes = match level {
            2 => TRAY_OVER,
            1 => TRAY_WARN,
            _ => TRAY_OK,
        };
        if let Ok(img) = Image::from_bytes(bytes) {
            let _ = tray.set_icon(Some(img));
        }
    }
}

// ---------------- alerts ----------------

fn check_alerts(app: &AppHandle, s: &Stats, cfg: &Settings) {
    let sess = s.current.as_ref().map(|c| c.id.clone()).unwrap_or_default();
    let checks = [
        ("Daily", &s.limits.daily, s.today_key.clone()),
        ("Monthly", &s.limits.monthly, s.month_key.clone()),
        ("Session", &s.limits.session, sess),
    ];
    let state = st(app);
    let mut alerted = state.alerted.lock().unwrap();
    for (name, lim, period) in checks {
        let Some(l) = lim else { continue };
        let key = format!("{name}:{period}");
        let prev = *alerted.get(&key).unwrap_or(&0);
        if l.level <= prev {
            continue;
        }
        alerted.insert(key, l.level);
        let c = &cfg.currency;
        let (title, body) = if l.level >= 2 {
            (
                format!("{name} limit reached"),
                format!("You've used {c}{:.2} of your {c}{:.2} {} limit.", l.used, l.limit, name.to_lowercase()),
            )
        } else {
            (
                format!("{name} limit at {:.0}%", l.pct),
                format!("{c}{:.2} of {c}{:.2} used. Tokeye will warn you again at 100%.", l.used, l.limit),
            )
        };
        if cfg.notify {
            let _ = app.notification().builder().title(&title).body(&body).show();
        }
        let _ = app.emit("alert", serde_json::json!({ "level": l.level, "title": title, "body": body }));
    }
}

// ---------------- background loops ----------------

fn publish(app: &AppHandle) {
    let state = st(app);
    let settings = state.settings.lock().unwrap().clone();
    let stats = state.engine.lock().unwrap().snapshot(&settings);
    update_tray(app, &stats, &settings);
    check_alerts(app, &stats, &settings);
    let v = serde_json::to_value(&stats).unwrap_or(serde_json::Value::Null);
    *state.stats.lock().unwrap() = v.clone();
    let _ = app.emit("stats", v);
}

fn engine_loop(app: AppHandle) {
    let mut last_publish = Instant::now() - Duration::from_secs(3600);
    loop {
        if let Some(paths) = st(&app).new_paths.lock().unwrap().take() {
            *st(&app).engine.lock().unwrap() = Engine::new(paths);
        }
        let settings = st(&app).settings.lock().unwrap().clone();
        let poll_ms = settings.poll_ms.clamp(500, 60_000);
        let days = settings.history_days.clamp(2, 3650) as i64;
        let changed = st(&app).engine.lock().unwrap().poll(days, &settings);
        let dirty = st(&app).dirty.swap(false, Ordering::SeqCst);
        if changed || dirty || last_publish.elapsed() > Duration::from_secs(30) {
            publish(&app);
            last_publish = Instant::now();
        }
        if st(&app).save_pending.swap(false, Ordering::SeqCst) {
            let state = st(&app);
            let s = state.settings.lock().unwrap().clone();
            let _ = s.save(&state.config_path);
        }
        let mut slept = 0;
        while slept < poll_ms {
            std::thread::sleep(Duration::from_millis(100));
            slept += 100;
            if st(&app).dirty.load(Ordering::SeqCst) {
                break;
            }
        }
    }
}

/// Streams the global cursor position (~30 fps) so the companions can look at it.
fn pointer_loop(app: AppHandle) {
    let mut last = (-1e9_f64, -1e9_f64);
    let mut tick: u64 = 0;
    loop {
        std::thread::sleep(Duration::from_millis(33));
        tick += 1;
        if let Ok(p) = app.cursor_position() {
            if (p.x - last.0).abs() > 0.5 || (p.y - last.1).abs() > 0.5 {
                last = (p.x, p.y);
                let _ = app.emit("pointer", (p.x, p.y));
            }
        }
        // The Windows taskbar raises itself when clicked; keep the strip (and
        // notch) above it. Doing this on Linux would repeatedly restack the
        // notch over the quick menu and steal its focus.
        if cfg!(windows) && tick % 9 == 0 {
            if let Some(w) = app.get_webview_window("taskbar") {
                raise(&w);
            }
        }
        if cfg!(windows) && tick % 60 == 0 {
            let floating_on_top = st(&app).settings.lock().unwrap().surface("floating").on_top;
            for label in ["notch", "floating"] {
                if label == "floating" && !floating_on_top {
                    continue;
                }
                if let Some(w) = app.get_webview_window(label) {
                    raise(&w);
                }
            }
        }
    }
}

// ---------------- commands ----------------

#[tauri::command]
fn get_stats(app: AppHandle) -> serde_json::Value {
    st(&app).stats.lock().unwrap().clone()
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Settings {
    st(&app).settings.lock().unwrap().clone()
}

#[tauri::command]
async fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    let state = st(&app);
    let old = state.settings.lock().unwrap().clone();
    let mut new = settings;
    // These are owned by Rust-side commands; a window's copy may be stale.
    new.positions = old.positions.clone();
    if new.surface("notch").anchor != "custom" {
        new.positions.remove("notch");
    }
    new.hidden_sessions = old.hidden_sessions.clone();
    new.clear_before = old.clear_before;
    new.first_run_done = true;
    new.save(&state.config_path).map_err(|e| e.to_string())?;
    *state.settings.lock().unwrap() = new.clone();
    if new.autostart != old.autostart {
        sync_autostart(&app, new.autostart);
    }
    let paths = resolve_paths(&app, &new);
    if paths != resolve_paths(&app, &old) {
        *state.new_paths.lock().unwrap() = Some(paths);
    }
    if new.daily_limit != old.daily_limit || new.monthly_limit != old.monthly_limit || new.session_limit != old.session_limit {
        state.alerted.lock().unwrap().clear();
    }
    apply_overlays(&app);
    if new.surfaces != old.surfaces {
        apply_surfaces(&app, &new);
    }
    state.dirty.store(true, Ordering::SeqCst);
    let _ = app.emit("settings", &new);
    Ok(())
}

fn update_settings(app: &AppHandle, f: impl FnOnce(&mut Settings)) {
    let state = st(app);
    let s = {
        let mut g = state.settings.lock().unwrap();
        f(&mut g);
        g.clone()
    };
    let _ = s.save(&state.config_path);
    state.dirty.store(true, Ordering::SeqCst);
    let _ = app.emit("settings", &s);
}

/// Hides sessions from every total. Transcripts on disk are left alone.
#[tauri::command]
fn hide_sessions(app: AppHandle, ids: Vec<String>) {
    update_settings(&app, |s| {
        for id in ids {
            if !id.is_empty() && !s.hidden_sessions.contains(&id) {
                s.hidden_sessions.push(id);
            }
        }
    });
}

/// Hides every session that only used Claude.ai / the Anthropic API. Returns how many were added.
#[tauri::command]
async fn hide_personal_sessions(app: AppHandle) -> usize {
    let ids = st(&app).engine.lock().unwrap().personal_session_ids();
    let mut added = 0;
    update_settings(&app, |s| {
        for id in ids {
            if !s.hidden_sessions.contains(&id) {
                s.hidden_sessions.push(id);
                added += 1;
            }
        }
    });
    added
}

/// Starts counting from now: everything earlier is ignored.
#[tauri::command]
fn clear_all_sessions(app: AppHandle) {
    let now = chrono::Local::now().timestamp();
    update_settings(&app, |s| s.clear_before = now);
}

#[tauri::command]
fn restore_sessions(app: AppHandle) {
    update_settings(&app, |s| {
        s.hidden_sessions.clear();
        s.clear_before = 0;
    });
}

#[tauri::command]
async fn bill_query(app: AppHandle, filter: engine::BillFilter) -> engine::BillResult {
    let s = st(&app).settings.lock().unwrap().clone();
    let r = st(&app).engine.lock().unwrap().bill(&s, &filter);
    r
}

/// Writes the filtered bill as CSV line items to the Downloads folder and returns the path.
#[tauri::command]
async fn export_bill(app: AppHandle, filter: engine::BillFilter) -> Result<String, String> {
    let s = st(&app).settings.lock().unwrap().clone();
    let csv = st(&app).engine.lock().unwrap().bill_csv(&s, &filter);
    let dir = app.path().download_dir().or_else(|_| app.path().home_dir()).map_err(|e| e.to_string())?;
    let path = dir.join(format!("tokeye-bill-{}.csv", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    std::fs::write(&path, csv).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// Shows a file in the system file manager.
#[tauri::command]
fn reveal_path(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(format!("Not found: {path}"));
    }
    #[cfg(windows)]
    let r = std::process::Command::new("explorer").arg(format!("/select,{}", p.display())).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg("-R").arg(&p).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(p.parent().unwrap_or(&p)).spawn();
    r.map(|_| ()).map_err(|e| e.to_string())
}

/// Opens a project folder with the editor command from settings ("code", "cursor", "zed", or a
/// custom command containing {path}).
#[tauri::command]
fn open_in_editor(app: AppHandle, path: String) -> Result<(), String> {
    let dir = PathBuf::from(path.trim());
    if path.trim().is_empty() || !dir.exists() {
        return Err(format!("Folder not found: {path}"));
    }
    let p = dir.display().to_string();
    if p.contains('"') {
        return Err("Unsupported folder name".into());
    }
    let editor = st(&app).settings.lock().unwrap().editor.trim().to_string();
    let editor = if editor.is_empty() { "code".to_string() } else { editor };
    let quoted = format!("\"{p}\"");
    let cmdline = if editor.contains("{path}") { editor.replace("{path}", &quoted) } else { format!("{editor} {quoted}") };
    #[cfg(windows)]
    let r = {
        use std::os::windows::process::CommandExt;
        // Editors like `code` are .cmd shims, so go through cmd; CREATE_NO_WINDOW avoids a console flash.
        std::process::Command::new("cmd").raw_arg(format!("/C \"{cmdline}\"")).creation_flags(0x0800_0000).spawn()
    };
    #[cfg(not(windows))]
    let r = std::process::Command::new("sh").arg("-c").arg(&cmdline).spawn();
    r.map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_click_through(window: WebviewWindow, ignore: bool) {
    // The Linux notch is sized to the pill and must always receive mouse input;
    // making it click-through would break hover again.
    let ignore = if cfg!(target_os = "linux") && window.label() == "notch" { false } else { ignore };
    let _ = window.set_ignore_cursor_events(ignore);
    raise(&window);
}

/// The notch page reports its state: whether it's open, and (for Linux) which parts of the
/// window take mouse input. The native window never resizes, so nothing gets clipped.
#[tauri::command]
fn set_notch_state(app: AppHandle, window: WebviewWindow, open: bool, rects: Vec<[f64; 4]>) {
    set_input_region(&app, &window, rects);
    // Don't restack while shrinking: the quick menu is anchored just below the
    // notch and must stay above it (raising the notch here stole its focus).
    if open {
        raise(&window);
    }
}

/// Long-press drag: moves the window with the cursor until the page calls `drag_end`
/// (Windows also watches the mouse button, in case the release never reaches the page).
#[tauri::command]
fn drag_start(app: AppHandle, window: WebviewWindow) {
    if st(&app).dragging.swap(true, Ordering::SeqCst) {
        return;
    }
    let (Ok(cur), Ok(start)) = (app.cursor_position(), window.outer_position()) else {
        st(&app).dragging.store(false, Ordering::SeqCst);
        return;
    };
    let off = (cur.x - start.x as f64, cur.y - start.y as f64);
    std::thread::spawn(move || {
        let began = Instant::now();
        let mut last = (start.x, start.y);
        while st(&app).dragging.load(Ordering::SeqCst) && began.elapsed() < Duration::from_secs(120) {
            #[cfg(windows)]
            if !win32::mouse_down() {
                break;
            }
            if let Ok(c) = app.cursor_position() {
                let next = ((c.x - off.0).round() as i32, (c.y - off.1).round() as i32);
                if next != last {
                    if window.set_position(Position::Physical(PhysicalPosition::new(next.0, next.1))).is_err() {
                        break;
                    }
                    last = next;
                }
            }
            std::thread::sleep(Duration::from_millis(8));
        }
        st(&app).dragging.store(false, Ordering::SeqCst);
        finish_drag(&app, &window);
    });
}

#[tauri::command]
fn drag_end(app: AppHandle) {
    st(&app).dragging.store(false, Ordering::SeqCst);
}

/// Keeps a dropped window on its screen, snaps the notch to the top edge when it's dropped
/// near it, and remembers the spot.
fn finish_drag(app: &AppHandle, w: &WebviewWindow) {
    let (Ok(pos), Ok(size)) = (w.outer_position(), w.outer_size()) else { return };
    let (ww, wh) = (size.width as i32, size.height as i32);
    let (mut x, mut y) = (pos.x, pos.y);
    let mon = app
        .monitor_from_point(x as f64 + ww as f64 / 2.0, y as f64 + 4.0)
        .ok()
        .flatten()
        .or_else(|| w.current_monitor().ok().flatten());
    if let Some(m) = mon {
        let (mx, my) = (m.position().x, m.position().y);
        let (mw, mh) = (m.size().width as i32, m.size().height as i32);
        x = x.clamp(mx, (mx + mw - ww).max(mx));
        y = y.clamp(my, (my + mh - wh).max(my));
        if w.label() == "notch" && y - my < (56.0 * m.scale_factor()) as i32 {
            y = my;
        }
    }
    if (x, y) != (pos.x, pos.y) {
        let _ = w.set_position(Position::Physical(PhysicalPosition::new(x, y)));
    }
    let label = w.label().to_string();
    let _ = app.emit_to(label.as_str(), "drag-done", ());
    update_settings(app, |s| {
        s.positions.insert(label.clone(), (x, y));
        if label == "notch" {
            s.surfaces.entry(label.clone()).or_default().anchor = "custom".into();
        }
    });
}

#[tauri::command]
async fn show_dashboard_cmd(app: AppHandle, tab: Option<String>) {
    hide(&app, "menu");
    show_dashboard(&app, None, tab.as_deref());
}

#[tauri::command]
fn hide_window(app: AppHandle, label: String) {
    hide(&app, &label);
}

#[tauri::command]
fn set_pinned(app: AppHandle, pinned: bool) {
    st(&app).pinned.store(pinned, Ordering::SeqCst);
}

#[tauri::command]
async fn open_menu_cmd(app: AppHandle, x: Option<f64>, y: Option<f64>, from: Option<String>) {
    let anchor = match (x, y) {
        (Some(x), Some(y)) => Some(PhysicalPosition::new(x, y)),
        _ => None,
    };
    open_menu(&app, anchor, from.as_deref());
}

#[tauri::command]
fn resize_self(window: WebviewWindow, width: f64, height: f64) {
    let _ = window.set_size(Size::Logical(LogicalSize::new(width, height)));
}

#[tauri::command]
async fn reset_positions(app: AppHandle) {
    update_settings(&app, |s| {
        s.positions.clear();
        if let Some(n) = s.surfaces.get_mut("notch") {
            if n.anchor == "custom" {
                n.anchor = "center".into();
            }
        }
    });
    let s = st(&app).settings.lock().unwrap().clone();
    for label in OVERLAYS {
        if let (Some(w), Some((x, y))) = (app.get_webview_window(label), default_overlay_pos(&app, label, &s.surface(label))) {
            let _ = w.set_position(Position::Physical(PhysicalPosition::new(x, y)));
        }
    }
}

#[tauri::command]
fn platform() -> &'static str {
    std::env::consts::OS
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

fn main() {
    // Wayland doesn't let clients position top-level windows or keep them above
    // others, so the notch/taskbar/bubble would show up as centered normal
    // windows. Run through XWayland when it's available, where all of that works.
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("GDK_BACKEND").is_none() && std::env::var_os("DISPLAY").is_some() {
            std::env::set_var("GDK_BACKEND", "x11");
        }
        // WebKitGTK's DMA-BUF renderer leaves windows blank on many NVIDIA, virtual-GPU and
        // older-driver setups. Shared-memory rendering is plenty for these small windows.
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    let autostarted = std::env::args().any(|a| a == "--autostart");
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_dashboard(app, None, None);
        }))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--autostart"])))
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            get_stats,
            get_settings,
            save_settings,
            set_click_through,
            set_notch_state,
            drag_start,
            drag_end,
            show_dashboard_cmd,
            hide_window,
            set_pinned,
            open_menu_cmd,
            resize_self,
            reset_positions,
            hide_sessions,
            hide_personal_sessions,
            clear_all_sessions,
            restore_sessions,
            bill_query,
            export_bill,
            reveal_path,
            open_in_editor,
            platform,
            quit
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let config_path = app.path().app_config_dir()?.join("settings.json");
            let mut settings = Settings::load(&config_path);
            let first_run = !settings.first_run_done;
            if first_run {
                settings.first_run_done = true;
                let _ = settings.save(&config_path);
            }
            let paths = resolve_paths(&handle, &settings);
            let autostart = settings.autostart;
            app.manage(AppState {
                settings: Mutex::new(settings),
                stats: Mutex::new(serde_json::Value::Null),
                engine: Mutex::new(Engine::new(paths)),
                config_path,
                alerted: Mutex::new(HashMap::new()),
                hidden_at: Mutex::new(HashMap::new()),
                tray_level: Mutex::new(0),
                new_paths: Mutex::new(None),
                pinned: AtomicBool::new(false),
                flyout: AtomicBool::new(false),
                dirty: AtomicBool::new(false),
                save_pending: AtomicBool::new(false),
                dragging: AtomicBool::new(false),
            });
            if first_run && autostart {
                sync_autostart(&handle, true);
            }
            build_tray(&handle)?;
            create_dashboard(&handle)?;
            create_menu(&handle)?;
            apply_overlays(&handle);
            let h = handle.clone();
            std::thread::spawn(move || engine_loop(h));
            let h = handle.clone();
            std::thread::spawn(move || pointer_loop(h));
            if !autostarted {
                show_dashboard(&handle, None, None);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Tokeye")
        .run(|_app, event| {
            if let RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
