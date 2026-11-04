import { invoke, listen, win, $, state, boot, money, countUp, setBar, primaryLimit, isWorking, bars, spark, trackPointer, moodFeeder, surface, METRICS, FIELDS } from './common.js';
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
const eyesEl = $('#eyes');
const eyes = new Eyes(eyesEl);
trackPointer(eyes, eyesEl);
const feedMood = moodFeeder(eyes);
let cfg = surface(mode);

function applySurface() {
  cfg = surface(mode);
  const root = $('#root');
  root.style.zoom = cfg.scale;
  root.style.setProperty('--radius', Math.max(0, cfg.radius ?? 8) + 'px');
  root.style.opacity = cfg.opacity * ((state.settings && state.settings.opacity) ?? 1);
  root.className = 'style-' + cfg.style;
  document.querySelectorAll('[data-f]').forEach(el => (el.hidden = !cfg.fields.includes(el.dataset.f)));
  // Changing the surface settings resizes the native notch window; keep it in
  // sync with whether the pill is currently expanded.
  const pill = $('#pill');
  if (isLinux && mode === 'notch' && pill) invoke('set_notch_open', { open: pill.classList.contains('open') });
}

const listFields = () => cfg.fields.filter(f => FIELDS[f]);

function render(st) {
  if (!st || !st.ready) return;
  const m = METRICS[cfg.metric] || METRICS.today;
  const lim = primaryLimit(st);
  const level = st.limits ? st.limits.level : 0;
  document.body.classList.toggle('working', isWorking(st));
  countUp($('#big'), m.v(st), m.fmt);
  countUp($('#big2'), m.v(st), m.fmt);
  const label = $('#bigLabel');
  if (label) label.textContent = m.label;
  const lt = $('#limitText');
  if (lt) lt.textContent = lim ? `of ${money(lim.limit)} · ${lim.pct.toFixed(0)}%` : 'no limit set';
  setBar($('#bar'), lim ? lim.pct : 0, level);

  const grid = $('#grid');
  if (grid) grid.innerHTML = listFields().map(f => `<div ${D}><span class="lbl" ${D}>${FIELDS[f].label}</span><b ${D}>${FIELDS[f].val(st)}</b></div>`).join('');
  const line = $('#line');
  if (line) {
    line.textContent = listFields()
      .map(f => (mode === 'taskbar' ? FIELDS[f].short + FIELDS[f].val(st) : `${FIELDS[f].val(st)} ${FIELDS[f].label.toLowerCase()}`))
      .join(mode === 'taskbar' ? ' ' : ' · ');
  }

  const pill = $('#pill');
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
    const r = $('#pill').getBoundingClientRect();
    const d = window.devicePixelRatio || 1;
    w.innerPosition()
      .then(pos => invoke('open_menu_cmd', {
        x: pos.x + (r.left + r.width / 2) * d,
        y: pos.y + (r.bottom + 6) * d,
      }))
      .catch(() => invoke('open_menu_cmd', { x: null, y: null }));
  } else {
    invoke('open_menu_cmd', { x: null, y: null });
  }
});
document.addEventListener('dblclick', e => {
  if (!e.target.closest('.eyes')) invoke('show_dashboard_cmd', { tab: null });
});

// Alerts: the companion panics and the notch shows a red banner.
let hovering = false;
let notchOpen = () => {};
let ignoring = true;
function setClickThrough(ignore) {
  // The Linux notch window is pill-sized and must always accept input.
  if (isLinux && mode === 'notch') return;
  if (ignore === ignoring) return;
  ignoring = ignore;
  invoke('set_click_through', { ignore });
}

listen('alert', e => {
  const a = e.payload;
  eyes.react(a.level >= 2 ? 'shocked' : 'worried', 2500);
  eyes.burst(a.level >= 2 ? ['!', '‼'] : ['!'], 3);
  const pill = $('#pill');
  if (pill) {
    $('#alert').textContent = `${a.title} — ${a.body}`;
    pill.classList.add('alerting');
    notchOpen(true);
    setClickThrough(false);
    clearTimeout(window._alertT);
    window._alertT = setTimeout(() => {
      pill.classList.remove('alerting');
      if (!hovering) notchOpen(false);
      setClickThrough(!hovering);
    }, 7000);
  }
});

// ---- notch: expand on hover ----
if (mode === 'notch') {
  const pill = $('#pill');
  let leaveT = null;

  notchOpen = open => {
    if (isLinux) invoke('set_notch_open', { open });
    pill.classList.toggle('open', open);
  };

  pill.addEventListener('click', e => {
    if (!e.target.closest('.eyes')) invoke('show_dashboard_cmd', { tab: null });
  });

  if (isLinux) {
    // The native input region is limited to the pill, so these are real pointer
    // events (the global cursor poll can't see the cursor under Wayland).
    pill.addEventListener('pointerenter', () => {
      hovering = true;
      clearTimeout(leaveT);
      if (cfg.expand) notchOpen(true);
    });
    pill.addEventListener('pointerleave', () => {
      hovering = false;
      clearTimeout(leaveT);
      leaveT = setTimeout(() => {
        if (hovering || pill.classList.contains('alerting')) return;
        notchOpen(false);
      }, 220);
    });
  } else {
    let pos = { x: 0, y: 0 };
    const refresh = () => w.innerPosition().then(p => (pos = p)).catch(() => {});
    refresh();
    setInterval(refresh, 3000);
    let leaveT = null;
    listen('pointer', e => {
      const [x, y] = e.payload;
      const r = pill.getBoundingClientRect();
      // getBoundingClientRect already includes CSS zoom; convert CSS px to physical px.
      const d = window.devicePixelRatio || 1;
      const pad = 4 * d;
      const L = pos.x + r.left * d, R = pos.x + r.right * d, T = pos.y + r.top * d, B = pos.y + r.bottom * d;
      const inside = x >= L - pad && x <= R + pad && y >= T - pad && y <= B + pad;
      if (inside && !hovering) {
        hovering = true;
        clearTimeout(leaveT);
        if (cfg.expand) pill.classList.add('open');
        setClickThrough(false);
      } else if (!inside && hovering) {
        hovering = false;
        clearTimeout(leaveT);
        leaveT = setTimeout(() => {
          if (hovering || pill.classList.contains('alerting')) return;
          pill.classList.remove('open');
          setClickThrough(true);
        }, 280);
      }
    });
  }
}
