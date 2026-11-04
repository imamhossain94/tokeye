// Shared helpers for every Tokeye window.
const T = window.__TAURI__;
export const invoke = (cmd, args) => T.core.invoke(cmd, args);
export const listen = (ev, fn) => T.event.listen(ev, fn);
export const win = () => T.window.getCurrentWindow();
export const $ = (sel, root = document) => root.querySelector(sel);
export const state = { stats: null, settings: null };

export const sym = () => (state.settings && state.settings.currency) || '$';

export function money(v, s = sym()) {
  if (v == null || isNaN(v)) v = 0;
  const a = Math.abs(v);
  if (a >= 10000) return s + (v / 1000).toFixed(1) + 'k';
  if (a >= 1000) return s + Math.round(v).toLocaleString('en-US');
  return s + v.toFixed(2);
}

export function tok(n) {
  n = n || 0;
  if (n >= 1e9) return (n / 1e9).toFixed(2) + 'B';
  if (n >= 1e6) return (n / 1e6).toFixed(1) + 'M';
  if (n >= 1e3) return (n / 1e3).toFixed(1) + 'k';
  return String(n);
}

export const inTok = t => (t ? (t.input || 0) + (t.cache_write || 0) + (t.cache_read || 0) : 0);

export function ago(ts) {
  if (!ts) return 'never';
  const s = Math.max(0, Date.now() / 1000 - ts);
  if (s < 60) return 'just now';
  if (s < 3600) return Math.floor(s / 60) + ' min ago';
  if (s < 86400) return Math.floor(s / 3600) + ' h ago';
  return Math.floor(s / 86400) + ' d ago';
}

export const isWorking = st => !!st && st.last_ingest > 0 && Date.now() / 1000 - st.last_ingest < 15;

export function primaryLimit(st) {
  const l = st && st.limits;
  if (!l) return null;
  return l.daily || l.monthly || l.session || null;
}

export function applySettings(s) {
  state.settings = s;
  const root = document.documentElement;
  const light = matchMedia('(prefers-color-scheme: light)').matches;
  root.dataset.theme = s.theme === 'system' ? (light ? 'light' : 'dark') : s.theme;
  root.style.setProperty('--accent', s.accent || '#D97757');
  root.style.setProperty('--eye', s.eye_color || '#9AF7FF');
  root.style.setProperty('--ov-opacity', s.opacity ?? 1);
  document.body.classList.toggle('no-companion', !s.companion);
  for (const e of eyesRegistry) e.configure(s);
}

/** Every Eyes instance in this window, so settings changes reach them. */
export const eyesRegistry = new Set();

export async function boot({ onStats, onSettings } = {}) {
  const [s, st] = await Promise.all([invoke('get_settings'), invoke('get_stats')]);
  applySettings(s);
  onSettings && onSettings(s);
  if (st && typeof st === 'object') {
    state.stats = st;
    onStats && onStats(st);
  }
  await listen('stats', e => {
    state.stats = e.payload;
    onStats && onStats(e.payload);
  });
  await listen('settings', e => {
    applySettings(e.payload);
    onSettings && onSettings(e.payload);
  });
  matchMedia('(prefers-color-scheme: light)').addEventListener('change', () => state.settings && applySettings(state.settings));
}

export async function saveSettings(patch) {
  const next = { ...state.settings, ...patch };
  state.settings = next;
  applySettings(next);
  await invoke('save_settings', { settings: next });
  return next;
}

/** Animates a number change in an element. */
export function countUp(el, to, fmt, ms = 650) {
  if (!el) return;
  const from = parseFloat(el.dataset.v || '0');
  el.dataset.v = to;
  if (!isFinite(from) || Math.abs(to - from) < 1e-9) {
    el.textContent = fmt(to);
    return;
  }
  const t0 = performance.now();
  const step = now => {
    const k = Math.min(1, (now - t0) / ms);
    const e = 1 - Math.pow(1 - k, 3);
    el.textContent = fmt(from + (to - from) * e);
    if (k < 1) requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
}

export function setBar(el, pct, level) {
  if (!el) return;
  el.style.width = Math.max(0, Math.min(100, pct || 0)).toFixed(1) + '%';
  const bar = el.parentElement;
  bar.classList.toggle('lv1', level === 1);
  bar.classList.toggle('lv2', level === 2);
}

/** Bar chart into an <svg viewBox="0 0 W H">. */
export function bars(svg, values, { limit = 0, highlightLast = true, warnPct = 80, gap = 0.28, title } = {}) {
  if (!svg) return;
  const vb = svg.viewBox.baseVal;
  const W = vb.width || 300, H = vb.height || 100;
  const max = Math.max(limit || 0, ...values, 1e-9);
  const bw = W / values.length;
  let out = '';
  values.forEach((v, i) => {
    const h = Math.max(v > 0 ? 1.5 : 0, (v / max) * (H - 4));
    let cls = 'chart-bar';
    if (highlightLast && i === values.length - 1) cls += ' today';
    if (limit > 0 && v >= limit) cls += ' over';
    else if (limit > 0 && v >= (limit * warnPct) / 100) cls += ' warn';
    const t = title ? `<title>${title(i, v)}</title>` : '';
    out += `<rect class="${cls}" x="${(i * bw + (bw * gap) / 2).toFixed(2)}" y="${(H - h).toFixed(2)}" width="${(bw * (1 - gap)).toFixed(2)}" height="${h.toFixed(2)}" rx="${Math.min(3, bw * 0.25).toFixed(2)}">${t}</rect>`;
  });
  if (limit > 0 && limit <= max) {
    const y = (H - (limit / max) * (H - 4)).toFixed(2);
    out += `<line class="chart-limit" x1="0" x2="${W}" y1="${y}" y2="${y}"/>`;
  }
  svg.innerHTML = out;
}

export function spark(svg, values) {
  if (!svg) return;
  const vb = svg.viewBox.baseVal;
  const W = vb.width || 140, H = vb.height || 28;
  const max = Math.max(...values, 1e-9);
  const pts = values.map((v, i) => [(i / Math.max(1, values.length - 1)) * W, H - 2 - (v / max) * (H - 4)]);
  const line = pts.map((p, i) => (i ? 'L' : 'M') + p[0].toFixed(1) + ' ' + p[1].toFixed(1)).join(' ');
  svg.innerHTML = `<path class="spark-area" d="${line} L${W} ${H} L0 ${H} Z"/><path class="spark-line" d="${line}"/>`;
}

/** Makes an Eyes instance follow the global cursor (positions are physical pixels). */
export function trackPointer(eyes, el) {
  eyesRegistry.add(eyes);
  if (state.settings) eyes.configure(state.settings);
  const w = win();
  let pos = { x: 0, y: 0 };
  const refresh = () => w.innerPosition().then(p => (pos = p)).catch(() => {});
  refresh();
  w.onMoved(e => (pos = e.payload)).catch(() => {});
  setInterval(refresh, 4000);
  listen('pointer', e => {
    const [x, y] = e.payload;
    const r = el.getBoundingClientRect();
    if (!r.width) return;
    const d = window.devicePixelRatio || 1;
    const cx = pos.x + (r.left + r.width / 2) * d;
    const cy = pos.y + (r.top + r.height / 2) * d;
    const dx = x - cx, dy = y - cy;
    eyes.lookAt(dx / (Math.abs(dx) + 240 * d), dy / (Math.abs(dy) + 180 * d), Math.hypot(dx, dy) / d);
  });
  return () => pos;
}

// ---- surface customization ----
export const SURFACE_DEFAULTS = {
  scale: 1, opacity: 1, metric: 'today', fields: ['in', 'out', 'month', 'limit', 'live'], style: 'solid',
  on_top: true, ghost: false, anchor: 'center', expand: true, chart_days: 14,
};
export function surface(label) {
  const s = (state.settings && state.settings.surfaces && state.settings.surfaces[label]) || {};
  return { ...SURFACE_DEFAULTS, ...s };
}

/** The big number a surface can show. */
export const METRICS = {
  today: { label: 'Today', v: st => st.today.cost, fmt: v => money(v) },
  month: { label: 'This month', v: st => st.month.cost, fmt: v => money(v) },
  session: { label: 'Session', v: st => (st.current ? st.current.cost : 0), fmt: v => money(v) },
  rate: { label: 'Last hour', v: st => st.last_hour_cost, fmt: v => money(v) },
  tokens_in: { label: 'Tokens in', v: st => inTok(st.today), fmt: v => tok(Math.round(v)) },
  tokens_out: { label: 'Tokens out', v: st => st.today.output, fmt: v => tok(Math.round(v)) },
  requests: { label: 'Requests', v: st => st.today.requests, fmt: v => String(Math.round(v)) },
};

/** Small extra values a surface can list. */
export const FIELDS = {
  in: { label: 'In', short: '↑', val: st => tok(inTok(st.today)) },
  out: { label: 'Out', short: '↓', val: st => tok(st.today.output) },
  today: { label: 'Today', short: 'D', val: st => money(st.today.cost) },
  month: { label: 'Month', short: 'M', val: st => money(st.month.cost) },
  session: { label: 'Session', short: 'S', val: st => money(st.current ? st.current.cost : 0) },
  requests: { label: 'Req', short: '#', val: st => String(st.today.requests) },
  rate: { label: '/hour', short: '⏱', val: st => money(st.last_hour_cost) },
};
export const FLAG_FIELDS = ['limit', 'chart', 'live'];

/** Feeds companion moods from stats (limit level, activity, cost jumps). */
export function moodFeeder(eyes) {
  let lastCost = null;
  return st => {
    if (!st || !st.ready) return;
    const cost = st.today ? st.today.cost : 0;
    eyes.setState({
      level: st.limits ? st.limits.level : 0,
      working: isWorking(st),
      idleMins: st.last_ts ? (Date.now() / 1000 - st.last_ts) / 60 : 999,
    });
    if (lastCost != null && cost > lastCost) {
      if (cost - lastCost >= 1) eyes.react('shocked', 1300);
      else eyes.hop();
    }
    lastCost = cost;
  };
}
