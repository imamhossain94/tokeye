import { invoke, listen, $, state, boot, money, tok, inTok, ago, countUp, setBar, isWorking, bars, saveSettings, trackPointer, moodFeeder, SURFACE_DEFAULTS } from './common.js';
import { Eyes } from './eyes.js';
import { initBill, refreshBill } from './bill.js';

const tEyes = new Eyes($('#tEyes'));
const hEyes = new Eyes($('#hEyes'));
new Eyes($('#lEyes'), { interactive: false }).setState({ working: true });
trackPointer(tEyes, $('#tEyes'));
trackPointer(hEyes, $('#hEyes'));
const feedT = moodFeeder(tEyes);
const feedH = moodFeeder(hEyes);
const esc = v => String(v).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

// ---- titlebar & tabs ----
let pinned = false;
$('#pin').onclick = () => {
  pinned = !pinned;
  $('#pin').classList.toggle('on', pinned);
  invoke('set_pinned', { pinned });
};
$('#min').onclick = () => invoke('hide_window', { label: 'dashboard' });
$('#close').onclick = () => invoke('hide_window', { label: 'dashboard' });
let currentTab = 'overview';
function goto(tab) {
  currentTab = tab;
  document.querySelectorAll('.tab').forEach(b => b.classList.toggle('active', b.dataset.tab === tab));
  document.querySelectorAll('.view').forEach(v => v.classList.toggle('active', v.id === 'v-' + tab));
  if (tab === 'bill') refreshBill();
}
initBill();
let billT = 0;
document.querySelectorAll('.tab').forEach(b => (b.onclick = () => goto(b.dataset.tab)));
listen('goto', e => goto(e.payload));
document.addEventListener('keydown', e => {
  if (e.key === 'Escape' && !pinned) invoke('hide_window', { label: 'dashboard' });
});

// ---- overview ----
function render(st) {
  if (!st || !st.ready) return;
  $('#loading').style.display = 'none';
  document.body.classList.toggle('working', isWorking(st));
  const t = st.today, L = st.limits || {};
  const s = state.settings || {};

  countUp($('#todayCost'), t.cost, v => money(v));
  const dl = L.daily;
  $('#todaySub').textContent = dl
    ? `${dl.pct.toFixed(0)}% of ${money(dl.limit)} daily limit · yesterday ${money(st.yesterday.cost)}`
    : `Yesterday ${money(st.yesterday.cost)} · no daily limit`;
  setBar($('#todayBar'), dl ? dl.pct : 0, dl ? dl.level : 0);

  $('#tin').textContent = tok(inTok(t));
  $('#tinSub').textContent = `${tok(t.input)} new · ${tok(t.cache_write)} cached · ${tok(t.cache_read)} re-read`;
  $('#tout').textContent = tok(t.output);
  $('#toutSub').textContent = `${t.requests} requests${t.web ? ` · ${t.web} web searches` : ''}`;

  const ml = L.monthly;
  countUp($('#month'), st.month.cost, v => money(v));
  $('#monthSub').textContent = `On pace for ${money(st.projection_month)}${ml ? ` · limit ${money(ml.limit)}` : ''}`;
  setBar($('#monthBar'), ml ? ml.pct : 0, ml ? ml.level : 0);

  const cur = st.current, sl = L.session;
  $('#sess').textContent = money(cur ? cur.cost : 0);
  $('#sessSub').textContent = cur ? `${cur.project || cur.tool} · ${ago(cur.last)}` : 'No session yet';
  setBar($('#sessBar'), sl ? sl.pct : 0, sl ? sl.level : 0);

  const worst = [['Daily', L.daily], ['Monthly', L.monthly], ['Session', L.session]].filter(([, l]) => l && l.level > 0).sort((a, b) => b[1].level - a[1].level)[0];
  const banner = $('#banner');
  banner.className = 'banner' + (worst ? ` show lv${worst[1].level}` : '');
  if (worst) {
    const [name, l] = worst;
    banner.textContent = l.level >= 2 ? `${name} limit reached: ${money(l.used)} of ${money(l.limit)}.` : `${name} spend is at ${l.pct.toFixed(0)}% of your ${money(l.limit)} limit.`;
  }

  const days = st.days.slice(-14);
  bars($('#days'), days.map(d => d.cost), {
    limit: s.daily_limit || 0,
    warnPct: s.warn_percent || 80,
    title: (i, v) => `${days[i].weekday} ${days[i].date}: ${money(v)} · ${tok(days[i].output)} out`,
  });
  $('#daysAxis').innerHTML = days.filter((_, i) => i % 3 === 0 || i === days.length - 1).map((d, i, a) => `<span>${i === a.length - 1 ? 'Today' : d.weekday}</span>`).join('');
  $('#daysSub').textContent = `last 14 days · 30-day avg ${money(st.days.reduce((a, d) => a + d.cost, 0) / 30)}`;

  bars($('#hours'), st.hours.map(h => h.cost), { highlightLast: false, gap: 0.2, title: (i, v) => `${i}:00 — ${money(v)}` });
  $('#hoursSub').textContent = `last hour ${money(st.last_hour_cost)}`;

  const chips = list => list.length ? list.map(p => `<span class="chip">${esc(p.label)}<b>${money(p.month.cost)}</b></span>`).join('') : '<span class="chip">No usage yet</span>';
  $('#tools').innerHTML = chips(st.tools);
  $('#providers').innerHTML = chips(st.providers) + (st.providers.some(p => p.key === 'anthropic') ? `<span class="chip">Claude.ai plan usage is shown at API prices</span>` : '');

  $('#foot').textContent = `${st.sources.filter(x => x.found && x.enabled).map(x => x.label).join(' · ') || 'No tools found'}`;
  $('#rate').textContent = st.out_last_5m ? `${tok(st.out_last_5m)} tokens out in 5 min` : '';

  renderProviderOptions(st);
  renderSources(st);
  renderModels(st);
  renderSessions(st);
  feedT(st);
  feedH(st);
  // Keep the bill live while it's open, without re-querying on every tick.
  if (currentTab === 'bill' && Date.now() - billT > 3000) {
    billT = Date.now();
    refreshBill();
  }
}

function modelTable(el, rows, key) {
  const list = rows.filter(m => m[key].requests > 0);
  el.innerHTML = list.length
    ? `<tr><th>Model</th><th>Req</th><th>In</th><th>Out</th><th>Cost</th></tr>` +
      list.map(m => `<tr><td class="m" title="${esc(m.model)}">${esc(m.model.replace(/^claude-/, ''))}</td><td>${m[key].requests}</td><td>${tok(inTok(m[key]))}</td><td>${tok(m[key].output)}</td><td><b>${money(m[key].cost)}</b></td></tr>`).join('')
    : `<tr><td class="empty">Nothing yet</td></tr>`;
}
function renderModels(st) {
  modelTable($('#mToday'), st.models, 'today');
  modelTable($('#mMonth'), st.models, 'month');
}

let clearArmedUntil = 0;
function renderSessions(st) {
  const cur = st.current && st.current.id;
  const armed = Date.now() < clearArmedUntil;
  const notes = [];
  if (st.clear_before > 0) notes.push(`Counting since ${new Date(st.clear_before * 1000).toLocaleString()}`);
  if (st.hidden_sessions > 0) notes.push(`${st.hidden_sessions} session${st.hidden_sessions === 1 ? '' : 's'} hidden (${money(st.hidden_cost)})`);
  $('#sessList').innerHTML = `
    <h3>Recent sessions <span>cost while loaded</span></h3>
    <div class="sess-actions">
      <button class="btn" data-act="personal" title="Hide every Claude Code session that only used your Claude.ai account">Hide Claude.ai sessions</button>
      <button class="btn danger ${armed ? 'armed' : ''}" data-act="all" title="Ignore all usage before now">${armed ? 'Click again to clear all' : 'Clear all'}</button>
    </div>
    ${st.sessions.length
      ? st.sessions.map(s => `
        <div class="sess ${s.id === cur && isWorking(st) ? 'active' : ''} ${s.dir ? 'openable' : ''}" ${s.dir ? `data-dir="${esc(s.dir)}" title="Open ${esc(s.dir)} in ${esc(editorName())}"` : ''}>
          <span class="s-dot"></span>
          <div class="s-main"><div class="s-name">${esc(s.project || 'Untitled')}<span class="tag">${esc(s.tool)}</span>${s.personal ? '<span class="tag">Claude.ai</span>' : ''}</div><div class="s-meta">${ago(s.last)} · ${s.requests} req · ${tok(inTok(s))} in · ${tok(s.output)} out</div></div>
          <b>${money(s.cost)}</b>
          ${s.dir ? `<button class="ico" data-open="${esc(s.dir)}" title="Open in ${esc(editorName())}">↗</button>` : ''}
          <button class="x" data-hide="${esc(s.id)}" title="Hide this session from Tokeye">×</button>
        </div>`).join('')
      : '<div class="empty">No sessions to show</div>'}
    ${notes.length ? `<div class="hidden-note">${notes.join(' · ')} · <a data-act="restore">Restore</a></div>` : ''}
    <div class="hidden-note" id="sessMsg" hidden></div>`;
}

function editorName() {
  const e = ((state.settings && state.settings.editor) || 'code').trim();
  const opt = [...document.querySelectorAll('#editorPreset option')].find(o => o.value === e);
  if (opt) return opt.textContent;
  if (['explorer', 'open', 'xdg-open'].includes(e)) return 'the file manager';
  return 'your editor';
}

function openDir(dir) {
  invoke('open_in_editor', { path: dir }).catch(err => {
    const m = $('#sessMsg');
    if (m) {
      m.hidden = false;
      m.textContent = `${err}. Check Settings → System → Code editor.`;
    }
  });
}

$('#sessList').addEventListener('click', async e => {
  const hide = e.target.closest('[data-hide]');
  if (hide) return invoke('hide_sessions', { ids: [hide.dataset.hide] });
  const open = e.target.closest('[data-open]') || (!e.target.closest('button, a') && e.target.closest('[data-dir]'));
  if (open) return openDir(open.dataset.open || open.dataset.dir);
  const act = e.target.closest('[data-act]');
  if (!act) return;
  if (act.dataset.act === 'personal') await invoke('hide_personal_sessions');
  else if (act.dataset.act === 'restore') await invoke('restore_sessions');
  else if (act.dataset.act === 'all') {
    if (Date.now() < clearArmedUntil) {
      clearArmedUntil = 0;
      await invoke('clear_all_sessions');
    } else {
      clearArmedUntil = Date.now() + 3000;
      setTimeout(() => state.stats && renderSessions(state.stats), 3100);
    }
  }
  if (state.stats) renderSessions(state.stats);
});

// ---- sources ----
let sourcesKey = '';
function renderSources(st) {
  const key = JSON.stringify(st.sources);
  if (key === sourcesKey) return;
  sourcesKey = key;
  $('#sources').innerHTML = st.sources.map(src => `
    <div class="row">
      <div class="r-l">${esc(src.label)}<small class="${src.found ? 'ok' : ''}">${src.found ? `Found · ${src.requests.toLocaleString()} requests loaded` : 'Not found on this PC'}${src.key === 'cursor' ? ' · put Cursor usage CSV exports in the import folder' : ''}</small></div>
      <label class="switch"><input type="checkbox" data-tool="${src.key}" ${src.enabled ? 'checked' : ''} /><span></span></label>
    </div>`).join('');
}
$('#sources').addEventListener('change', e => {
  const el = e.target.closest('[data-tool]');
  if (el) saveSettings({ tools: { ...state.settings.tools, [el.dataset.tool]: el.checked } });
});

let provKey = '';
function renderProviderOptions(st) {
  const keys = st.providers.map(p => p.key + '|' + p.label).join(',');
  if (keys === provKey) return;
  provKey = keys;
  const sel = $('#provSelect');
  const cur = (state.settings && state.settings.providers) || 'all';
  const opts = [['all', 'Everything'], ...st.providers.map(p => [p.key, p.label])];
  if (!opts.some(([k]) => k === cur)) opts.push([cur, cur]);
  sel.innerHTML = opts.map(([k, l]) => `<option value="${esc(k)}">${esc(l)}</option>`).join('');
  sel.value = cur;
}

// ---- surfaces editor ----
const SHOW_KEY = { notch: 'show_notch', floating: 'show_floating', widget: 'show_widget', taskbar: 'show_taskbar' };
const SURF_NAME = { notch: 'Show the notch island', floating: 'Show the floating bubble', widget: 'Show the desktop widget', taskbar: 'Show the taskbar strip' };
const FIELD_CHIPS = [['in', 'Tokens in'], ['out', 'Tokens out'], ['today', 'Today'], ['month', 'Month'], ['session', 'Session'], ['requests', 'Requests'], ['rate', 'Last hour'], ['limit', 'Limit bar'], ['chart', 'Chart'], ['live', 'Live dot']];
let surf = 'notch';
const curSurface = () => ({ ...SURFACE_DEFAULTS, ...((state.settings && state.settings.surfaces && state.settings.surfaces[surf]) || {}) });
function saveSurface(patch) {
  const all = { ...(state.settings.surfaces || {}) };
  all[surf] = { ...curSurface(), ...patch };
  saveSettings({ surfaces: all });
}
function fillSurface() {
  if (!state.settings) return;
  const c = curSurface();
  document.querySelectorAll('#surfTabs button').forEach(b => b.classList.toggle('active', b.dataset.s === surf));
  $('#sfName').textContent = SURF_NAME[surf];
  $('#sfShow').checked = !!state.settings[SHOW_KEY[surf]];
  document.querySelectorAll('[data-sf]').forEach(el => {
    if (el === document.activeElement && el.type !== 'checkbox') return;
    const v = c[el.dataset.sf];
    if (el.type === 'checkbox') el.checked = !!v;
    else el.value = v ?? '';
  });
  $('#sfRadiusVal').textContent = `${Math.round(c.radius ?? 8)} px`;
  $('#sfFields').innerHTML = FIELD_CHIPS.map(([k, l]) => `<button class="fchip ${c.fields.includes(k) ? 'on' : ''}" data-field="${k}">${l}</button>`).join('');
  document.querySelectorAll('.sf-notch').forEach(r => (r.hidden = surf !== 'notch'));
  document.querySelectorAll('.sf-float').forEach(r => (r.hidden = !['floating', 'widget'].includes(surf)));
  document.querySelectorAll('.sf-widget').forEach(r => (r.hidden = surf !== 'widget'));
}
$('#surfTabs').addEventListener('click', e => {
  const b = e.target.closest('[data-s]');
  if (!b) return;
  surf = b.dataset.s;
  fillSurface();
});
$('#sfShow').addEventListener('change', e => saveSettings({ [SHOW_KEY[surf]]: e.target.checked }));
$('#sfFields').addEventListener('click', e => {
  const b = e.target.closest('[data-field]');
  if (!b) return;
  const fields = new Set(curSurface().fields);
  fields.has(b.dataset.field) ? fields.delete(b.dataset.field) : fields.add(b.dataset.field);
  saveSurface({ fields: FIELD_CHIPS.map(([k]) => k).filter(k => fields.has(k)) });
});
let sfT = null;
document.querySelectorAll('[data-sf]').forEach(el => {
  const instant = el.type === 'checkbox' || el.tagName === 'SELECT';
  el.addEventListener(instant ? 'change' : 'input', () => {
    let v = el.type === 'checkbox' ? el.checked : el.value;
    if (el.type === 'range' || el.dataset.sf === 'chart_days') v = parseFloat(v);
    if (el.dataset.sf === 'radius') $('#sfRadiusVal').textContent = `${v} px`;
    clearTimeout(sfT);
    sfT = setTimeout(() => saveSurface({ [el.dataset.sf]: v }), instant ? 0 : 250);
  });
});

// ---- general settings ----
const ACCENTS = ['#D97757', '#7C6CF2', '#3B82F6', '#10B981', '#F59E0B', '#EC4899', '#A3A3A3'];
const EYES = ['#9AF7FF', '#FFFFFF', '#B9FBC0', '#FFD6A5', '#FFADAD', '#CDB4DB'];
function swatches(el, colors, key) {
  el.innerHTML = colors.map(c => `<span class="sw" style="background:${c}" data-c="${c}"></span>`).join('');
  el.onclick = e => {
    const c = e.target.dataset.c;
    if (c) saveSettings({ [key]: c });
  };
}
swatches($('#accents'), ACCENTS, 'accent');
swatches($('#eyeColors'), EYES, 'eye_color');

function fillSettings(s) {
  document.querySelectorAll('[data-k]').forEach(el => {
    const v = s[el.dataset.k];
    if (el === document.activeElement) return;
    if (el.type === 'checkbox') el.checked = !!v;
    else el.value = v ?? '';
  });
  document.querySelectorAll('[data-path]').forEach(el => {
    if (el !== document.activeElement) el.value = (s.tool_paths && s.tool_paths[el.dataset.path]) || '';
  });
  document.querySelectorAll('#accents .sw').forEach(x => x.classList.toggle('on', x.dataset.c.toLowerCase() === (s.accent || '').toLowerCase()));
  document.querySelectorAll('#eyeColors .sw').forEach(x => x.classList.toggle('on', x.dataset.c.toLowerCase() === (s.eye_color || '').toLowerCase()));
  if (document.activeElement !== $('#prices')) $('#prices').value = JSON.stringify(s.prices, null, 1);
  fillEditor(s);
  fillSurface();
}

// ---- code editor ----
const FILE_MANAGER = { windows: 'explorer', macos: 'open', linux: 'xdg-open' };
let osName = 'windows';
function fillEditor(s) {
  const e = (s.editor || 'code').trim();
  const presets = [...document.querySelectorAll('#editorPreset option')].map(o => o.value);
  let preset = presets.includes(e) && e !== 'custom' && e !== 'files' ? e : 'custom';
  if (Object.values(FILE_MANAGER).includes(e)) preset = 'files';
  if (document.activeElement !== $('#editorPreset')) $('#editorPreset').value = preset;
  $('#editorCustomRow').hidden = preset !== 'custom';
  if (preset === 'custom' && document.activeElement !== $('#editorCustom')) $('#editorCustom').value = e;
}
$('#editorPreset').addEventListener('change', e => {
  const v = e.target.value;
  $('#editorCustomRow').hidden = v !== 'custom';
  if (v === 'custom') return $('#editorCustom').focus();
  saveSettings({ editor: v === 'files' ? FILE_MANAGER[osName] || 'explorer' : v });
});
$('#editorCustom').addEventListener('input', () => {
  clearTimeout(saveT);
  saveT = setTimeout(() => saveSettings({ editor: $('#editorCustom').value.trim() }), 600);
});

let saveT = null;
document.querySelectorAll('[data-k]').forEach(el => {
  const instant = el.type === 'checkbox' || el.tagName === 'SELECT';
  el.addEventListener(instant ? 'change' : 'input', () => {
    const k = el.dataset.k;
    let v = el.type === 'checkbox' ? el.checked : el.value;
    if (el.type === 'number' || el.type === 'range') v = parseFloat(v) || 0;
    if (['poll_ms', 'history_days'].includes(k)) v = Math.round(v);
    clearTimeout(saveT);
    saveT = setTimeout(() => saveSettings({ [k]: v }), instant ? 0 : 450);
  });
});
document.querySelectorAll('[data-path]').forEach(el =>
  el.addEventListener('input', () => {
    clearTimeout(saveT);
    saveT = setTimeout(() => saveSettings({ tool_paths: { ...(state.settings.tool_paths || {}), [el.dataset.path]: el.value.trim() } }), 600);
  })
);
$('#prices').addEventListener('input', () => {
  clearTimeout(saveT);
  saveT = setTimeout(() => {
    try {
      const p = JSON.parse($('#prices').value);
      if (Array.isArray(p)) {
        $('#prices').style.borderColor = '';
        saveSettings({ prices: p });
      }
    } catch {
      $('#prices').style.borderColor = 'var(--bad)';
    }
  }, 700);
});
$('#resetPos').onclick = () => invoke('reset_positions');
$('#quit').onclick = () => invoke('quit');

invoke('platform').then(os => {
  osName = os;
  document.documentElement.dataset.os = os;
  $('#autoLabel').textContent = os === 'windows' ? 'Start with Windows' : 'Start at login';
});

boot({ onStats: render, onSettings: s => { fillSettings(s); state.stats && render(state.stats); } });
setInterval(() => state.stats && document.body.classList.toggle('working', isWorking(state.stats)), 3000);
