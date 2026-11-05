import { invoke, listen, win, currentMonitor, $, state, boot, money, countUp, setBar, primaryLimit, isWorking, bars, spark, trackPointer, moodFeeder, surface, view, viewLabel, periodTotals, METRICS, FIELDS } from './common.js';
import { Eyes } from './eyes.js';

const w = win();
const mode = w.label; // notch | floating | widget | taskbar
const isLinux = /Linux/.test(navigator.userAgent);
document.body.dataset.mode = mode;
const D = 'data-tauri-drag-region';

const templates = {
  notch: `
    <div class="notch" id="pill">
      <div class="n-eyes" id="eyes"></div>
      <div class="n-mini"><span class="live" data-f="live"></span><b id="big">$0.00</b></div>
      <div class="n-full">
        <div class="n-head"><span class="lbl" id="bigLabel">Today</span><b class="big" id="big2">$0.00</b><span class="sub" id="limitText" data-f="limit"></span></div>
        <div class="bar" data-f="limit"><i id="bar"></i></div>
        <div class="n-grid" id="grid"></div>
        <svg class="n-spark" id="spark" data-f="chart" viewBox="0 0 240 30" preserveAspectRatio="none"></svg>
      </div>
      <div class="n-alert" id="alert"></div>
    </div>`,
  floating: `
    <div class="float card-s" ${D}>
      <div class="f-eyes" id="eyes"></div>
      <div class="f-body" ${D}>
        <div class="f-top" ${D}><span class="lbl" id="bigLabel" ${D}>Today</span><span class="live" data-f="live"></span></div>
        <b class="big" id="big" ${D}>$0.00</b>
        <div class="f-sub" id="line" ${D}></div>
        <div class="bar" data-f="limit"><i id="bar"></i></div>
      </div>
    </div>`,
  widget: `
    <div class="widget card-s" ${D}>
      <header ${D}>
        <div class="w-eyes" id="eyes"></div>
        <div ${D}><div class="lbl" ${D}><span id="bigLabel" ${D}>Today</span> <span class="live" data-f="live"></span></div><b class="big" id="big" ${D}>$0.00</b></div>
      </header>
      <div class="bar" data-f="limit"><i id="bar"></i></div>
      <div class="w-stats" id="grid" ${D}></div>
      <svg class="w-chart" id="chart" data-f="chart" viewBox="0 0 300 56" preserveAspectRatio="none"></svg>
    </div>`,
  taskbar: `
    <div class="strip card-s" ${D}>
      <div class="t-eyes" id="eyes"></div>
      <b id="big" ${D}>$0.00</b>
      <span class="t-sub" id="line" ${D}></span>
      <span class="live" data-f="live"></span>
      <i class="bar t-bar" data-f="limit"><i id="bar"></i></i>
    </div>`,
};

$('#root').innerHTML = templates[mode] || templates.floating;
const pill = $('#pill'); // notch only
const eyesEl = $('#eyes');
const eyes = new Eyes(eyesEl);
trackPointer(eyes, eyesEl);
const feedMood = moodFeeder(eyes);
let cfg = surface(mode);

// ---- state shared by the interactions below ----
let hovering = false; // pointer over the surface (the pill, for the notch)
let open = false; // notch expanded
let hidden = false; // auto-hidden
let pressing = false; // mouse held on the notch, long press not reached yet
let dragging = false; // long-press drag in progress
let lastActive = Date.now();
const touch = () => (lastActive = Date.now());
const alerting = () => !!pill && pill.classList.contains('alerting');

// Window and screen geometry in physical px, to compare with the global cursor.
let pos = { x: 0, y: 0 };
let mon = null;
let geomT = 0;
function refreshGeometry() {
  w.innerPosition().then(p => (pos = p)).catch(() => {});
  currentMonitor()
    .then(m => {
      if (m) mon = { x: m.position.x, y: m.position.y, w: m.size.width, h: m.size.height };
      // A notch dragged away from the top edge gets rounded corners all around.
      if (pill) document.body.classList.toggle('detached', !!mon && pos.y > mon.y + 2);
    })
    .catch(() => {});
}
refreshGeometry();
setInterval(refreshGeometry, 5000);
w.onMoved(e => {
  pos = e.payload;
  clearTimeout(geomT);
  geomT = setTimeout(refreshGeometry, 200);
}).catch(() => {});

// Click-through: Rust starts the notch (except on Linux) and ghost surfaces click-through.
let ignoring = mode === 'notch' ? !isLinux : null;
const baseIgnore = () => mode !== 'notch' && mode !== 'taskbar' && !!cfg.ghost;
function setClickThrough(ignore, force = false) {
  // The Linux notch uses an input region instead (see reportNotch).
  if (isLinux && mode === 'notch') return;
  if (!force && ignore === ignoring) return;
  ignoring = ignore;
  invoke('set_click_through', { ignore });
}

// ---- notch shape ----
// Target sizes of the pill, matching .notch and .notch.open in app.css (it animates between them).
const PILL = { closed: [204, 36], open: [440, 176] };
function notchRects() {
  const vw = document.documentElement.clientWidth;
  if (hidden) return [[0, 0, vw, 3]]; // a strip along the top edge brings it back
  const k = cfg.scale || 1;
  const [pw, ph] = open ? PILL.open : PILL.closed;
  return [[(vw - pw * k) / 2, 0, pw * k, ph * k]];
}
/** Tells Rust the notch's state: raises it when it opens, and sets the Linux input region. */
function reportNotch() {
  if (pill) invoke('set_notch_state', { open, rects: isLinux ? notchRects() : [] });
}
function setOpen(v) {
  if (!pill || v === open) return;
  open = v;
  pill.classList.toggle('open', v);
  reportNotch();
}
const expandsOnHover = () => cfg.expand && cfg.expand_on !== 'click';
const expandsOnClick = () => cfg.expand && cfg.expand_on === 'click';

function applySurface() {
  cfg = surface(mode);
  const root = $('#root');
  root.style.zoom = cfg.scale;
  root.style.setProperty('--radius', Math.max(0, cfg.radius ?? 8) + 'px');
  root.style.opacity = cfg.opacity * ((state.settings && state.settings.opacity) ?? 1);
  root.className = 'style-' + cfg.style;
  document.querySelectorAll('[data-f]').forEach(el => (el.hidden = !cfg.fields.includes(el.dataset.f)));
  if (pill) {
    if (!cfg.expand && open && !alerting()) setOpen(false);
    reportNotch(); // the size may have changed
  }
  if (hidden && !(cfg.auto_hide > 0)) reveal();
  // Saving settings resets click-through on the bubble and widget; keep a hidden one click-through.
  else if (hidden) setClickThrough(true, true);
  touch();
}

const listFields = () => cfg.fields.filter(f => FIELDS[f]);

function render(st) {
  if (!st || !st.ready) return;
  // A newly pinned scope arrives with the next stats; keep the old numbers until then.
  if (cfg.scope && !(st.scopes && st.scopes[cfg.scope])) return;
  const v = view(st, cfg);
  const m = METRICS[v.metric];
  const P = periodTotals(v.st, v.period);
  const lim = primaryLimit(st);
  const level = st.limits ? st.limits.level : 0;
  document.body.classList.toggle('working', isWorking(st));
  document.body.classList.toggle('scoped', !!v.sc);
  countUp($('#big'), m.v(P, v.st), m.fmt);
  countUp($('#big2'), m.v(P, v.st), m.fmt);
  const label = $('#bigLabel');
  if (label) {
    label.textContent = viewLabel(v);
    label.title = v.sc && v.sc.dir ? v.sc.dir : '';
  }
  const lt = $('#limitText');
  if (lt) lt.textContent = lim ? `of ${money(lim.limit)} · ${lim.pct.toFixed(0)}%` : 'no limit set';
  setBar($('#bar'), lim ? lim.pct : 0, level);

  const grid = $('#grid');
  if (grid) grid.innerHTML = listFields().map(f => `<div ${D}><span class="lbl" ${D}>${FIELDS[f].label}</span><b ${D}>${FIELDS[f].val(v.st, P)}</b></div>`).join('');
  const line = $('#line');
  if (line) {
    line.textContent = listFields()
      .map(f => (mode === 'taskbar' ? FIELDS[f].short + FIELDS[f].val(v.st, P) : `${FIELDS[f].val(v.st, P)} ${FIELDS[f].label.toLowerCase()}`))
      .join(mode === 'taskbar' ? ' ' : ' · ');
  }

  if (pill) {
    pill.classList.toggle('lv1', level === 1);
    pill.classList.toggle('lv2', level === 2);
  }
  if ($('#spark')) spark($('#spark'), st.hours.map(h => h.cost));
  if ($('#chart')) {
    const days = st.days.slice(-Math.max(3, Math.min(30, cfg.chart_days || 14)));
    bars($('#chart'), days.map(d => d.cost), { limit: state.settings ? state.settings.daily_limit : 0, warnPct: state.settings ? state.settings.warn_percent : 80 });
  }
  feedMood(st);
}

boot({
  onStats: render,
  onSettings: () => {
    applySurface();
    if (state.stats) render(state.stats);
  },
});

// Every surface: right-click opens the custom menu, double-click opens the dashboard.
document.addEventListener('contextmenu', e => {
  e.preventDefault();
  if (isLinux && mode === 'notch') {
    // Anchor the menu just below the pill instead of on top of it, so the menu
    // doesn't cover the notch (and the pointer doesn't immediately leave it).
    const r = pill.getBoundingClientRect();
    const d = window.devicePixelRatio || 1;
    w.innerPosition()
      .then(p => invoke('open_menu_cmd', { x: p.x + (r.left + r.width / 2) * d, y: p.y + (r.bottom + 6) * d, from: mode }))
      .catch(() => invoke('open_menu_cmd', { x: null, y: null, from: mode }));
  } else {
    invoke('open_menu_cmd', { x: null, y: null, from: mode });
  }
});
document.addEventListener('dblclick', e => {
  if (!e.target.closest('.eyes')) invoke('show_dashboard_cmd', { tab: null });
});

// ---- alerts: the companion panics, the surface shows itself, the notch shows a red banner ----
let alertT = 0;
listen('alert', e => {
  const a = e.payload;
  reveal();
  touch();
  eyes.react(a.level >= 2 ? 'shocked' : 'worried', 2500);
  eyes.burst(a.level >= 2 ? ['!', '‼'] : ['!'], 3);
  if (!pill) return;
  const wasOpen = open && !alerting();
  $('#alert').textContent = `${a.title} — ${a.body}`;
  pill.classList.add('alerting');
  setOpen(true);
  setClickThrough(false);
  clearTimeout(alertT);
  alertT = setTimeout(() => {
    pill.classList.remove('alerting');
    if (!hovering) setOpen(wasOpen && expandsOnClick());
    setClickThrough(!hovering);
    touch();
  }, 7000);
});

// ---- auto-hide: after a while without the pointer; touching the top screen edge brings it back ----
function hide() {
  if (hidden) return;
  hidden = true;
  document.body.classList.add('auto-hidden');
  if (pill) {
    open = false;
    pill.classList.remove('open');
    reportNotch();
  }
  setClickThrough(true, true);
}
function reveal() {
  if (!hidden) return;
  hidden = false;
  touch();
  document.body.classList.remove('auto-hidden');
  if (pill) reportNotch();
  setClickThrough(pill ? !hovering : baseIgnore(), true);
  eyes.react('happy', 900);
  eyes.hop();
}
setInterval(() => {
  if (hidden || !(cfg.auto_hide > 0) || hovering || pressing || dragging || alerting()) return;
  if (Date.now() - lastActive >= cfg.auto_hide * 1000) hide();
}, 250);

/** The cursor touches the top edge of this surface's screen (above the notch only, if set so). */
function edgeTouched(x, y) {
  if (!mon || y > mon.y + 1 || x < mon.x || x >= mon.x + mon.w) return false;
  if (pill && cfg.reveal === 'notch') {
    const d = window.devicePixelRatio || 1;
    return x >= pos.x && x <= pos.x + window.innerWidth * d;
  }
  return true;
}

// ---- hover ----
let leaveT = 0;
function enter() {
  hovering = true;
  clearTimeout(leaveT);
  touch();
  if (pill && expandsOnHover()) setOpen(true);
}
function leave() {
  hovering = false;
  touch();
  clearTimeout(leaveT);
  if (!pill) return;
  leaveT = setTimeout(() => {
    if (hovering || dragging || alerting()) return;
    if (!expandsOnClick()) setOpen(false);
    setClickThrough(true);
  }, isLinux ? 220 : 280);
}

/** Cursor over the pill (+ a few px), from the global cursor stream. */
function overPill(x, y) {
  const r = pill.getBoundingClientRect();
  const d = window.devicePixelRatio || 1;
  const pad = 4 * d;
  return x >= pos.x + r.left * d - pad && x <= pos.x + r.right * d + pad && y >= pos.y + r.top * d - pad && y <= pos.y + r.bottom * d + pad;
}

listen('pointer', e => {
  const [x, y] = e.payload;
  if (dragging) return;
  if (hidden) {
    if (edgeTouched(x, y)) reveal();
    return;
  }
  // Linux gets real pointer events over the notch's input region instead (handled below).
  if (!pill || isLinux) return;
  const inside = overPill(x, y);
  if (inside && !hovering) {
    setClickThrough(false);
    enter();
  } else if (!inside && hovering) {
    leave();
  }
});

if (pill && isLinux) {
  pill.addEventListener('pointerenter', enter);
  pill.addEventListener('pointerleave', leave);
  // While hidden, only a thin strip along the top takes input; touching it brings the notch back.
  document.addEventListener('pointermove', () => hidden && reveal());
} else if (!pill) {
  document.documentElement.addEventListener('pointerenter', enter);
  document.documentElement.addEventListener('pointerleave', leave);
}

// ---- notch: click to expand, long press to move ----
if (pill) {
  const LONG_PRESS_MS = 450;
  let pressT = 0;
  let pressAt = null;
  let suppressClick = false;

  // Capture phase, so this runs before the eyes' own click handler (boop).
  pill.addEventListener(
    'click',
    e => {
      if (suppressClick) {
        suppressClick = false;
        e.stopPropagation();
        return;
      }
      touch();
      if (expandsOnClick()) {
        // The second click of a double-click opens the dashboard instead.
        if (e.detail > 1) return;
        // Open: a click anywhere opens it. Already open: the eyes keep their boop.
        if (open && e.target.closest('.eyes')) return;
        e.stopPropagation();
        setOpen(!open);
      } else if (!e.target.closest('.eyes')) {
        invoke('show_dashboard_cmd', { tab: null });
      }
    },
    true
  );

  pill.addEventListener('pointerdown', e => {
    if (e.button !== 0) return;
    pressing = true;
    pressAt = [e.screenX, e.screenY];
    try {
      pill.setPointerCapture(e.pointerId);
    } catch {}
    clearTimeout(pressT);
    pressT = setTimeout(() => {
      if (!pressing) return;
      pressing = false;
      dragging = true;
      suppressClick = true;
      document.body.classList.add('dragging');
      eyes.react('happy', 1500);
      eyes.hop();
      invoke('drag_start');
    }, LONG_PRESS_MS);
  });
  pill.addEventListener('pointermove', e => {
    // Moving before the long press completes is neither a drag nor a click-and-hold.
    if (pressing && pressAt && Math.hypot(e.screenX - pressAt[0], e.screenY - pressAt[1]) > 6) {
      pressing = false;
      clearTimeout(pressT);
    }
  });
  const release = () => {
    pressing = false;
    clearTimeout(pressT);
    if (!dragging) return;
    dragging = false;
    document.body.classList.remove('dragging');
    invoke('drag_end');
    touch();
    // The click that follows the release was part of the drag.
    setTimeout(() => (suppressClick = false), 300);
  };
  pill.addEventListener('pointerup', release);
  pill.addEventListener('pointercancel', release);
  pill.addEventListener('lostpointercapture', release);
  // Rust ends the drag on its own when the button is released outside the page's view.
  listen('drag-done', release);
}
