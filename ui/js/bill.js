// The Bill tab: filter spend by period, provider, tool, project, session and model, group it, export it.
import { invoke, $, money, tok, inTok, bars } from './common.js';

const esc = v => String(v).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
const iso = d => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
const LABELS = { provider: 'Billed to', tool: 'Tool', project: 'Project', session: 'Session', model: 'Model' };
const LISTS = { provider: 'providers', tool: 'tools', project: 'projects', session: 'sessions', model: 'models' };
// Clicking a row filters by it and shows the next level down.
const DRILL = { project: ['project', 'session'], session: ['session', 'model'], provider: ['provider', 'project'], tool: ['tool', 'project'], model: ['model', 'session'] };

const F = { period: 'month', from: '', to: '', provider: '', tool: '', project: '', session: '', model: '', group: 'project', include_hidden: false };
let busy = false;
let pending = false;

function range() {
  const now = new Date();
  const t0 = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const sec = d => Math.floor(d.getTime() / 1000);
  const add = (d, n) => new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
  switch (F.period) {
    case 'today': return [sec(t0), 0, 'Today'];
    case 'yesterday': return [sec(add(t0, -1)), sec(t0), 'Yesterday'];
    case '7d': return [sec(add(t0, -6)), 0, 'Last 7 days'];
    case '30d': return [sec(add(t0, -29)), 0, 'Last 30 days'];
    case 'last_month': {
      const a = new Date(now.getFullYear(), now.getMonth() - 1, 1);
      return [sec(a), sec(new Date(now.getFullYear(), now.getMonth(), 1)), a.toLocaleString(undefined, { month: 'long', year: 'numeric' })];
    }
    case 'all': return [0, 0, 'Everything loaded'];
    case 'custom': {
      const a = F.from ? new Date(F.from + 'T00:00:00') : null;
      const b = F.to ? add(new Date(F.to + 'T00:00:00'), 1) : null;
      return [a ? sec(a) : 0, b ? sec(b) : 0, F.from === F.to && F.from ? F.from : `${F.from || '…'} → ${F.to || 'now'}`];
    }
    default: return [sec(new Date(now.getFullYear(), now.getMonth(), 1)), 0, 'This month'];
  }
}

function filter() {
  const [from, to] = range();
  return { from, to, provider: F.provider, tool: F.tool, project: F.project, session: F.session, model: F.model, group: F.group, include_hidden: F.include_hidden };
}

export async function refreshBill() {
  if (busy) {
    pending = true;
    return;
  }
  busy = true;
  try {
    render(await invoke('bill_query', { filter: filter() }));
  } catch (e) {
    $('#billFoot').textContent = 'Could not load the bill: ' + e;
  } finally {
    busy = false;
    if (pending) {
      pending = false;
      refreshBill();
    }
  }
}

function fillSelect(key, list, allLabel) {
  const sel = document.querySelector(`[data-bf="${key}"]`);
  const cur = F[key];
  const opts = [['', allLabel], ...list.map(o => [o.key, `${o.label} — ${money(o.cost)}`])];
  if (cur && !list.some(o => o.key === cur)) opts.push([cur, cur]);
  sel.innerHTML = opts.map(([k, l]) => `<option value="${esc(k)}">${esc(l)}</option>`).join('');
  sel.value = cur;
}

function render(r) {
  const [, , title] = range();
  document.querySelectorAll('#billPeriod button').forEach(b => b.classList.toggle('active', b.dataset.p === F.period));
  document.querySelectorAll('#billGroup button').forEach(b => b.classList.toggle('active', b.dataset.g === F.group));
  $('#billCustom').hidden = F.period !== 'custom';
  fillSelect('provider', r.providers, 'All providers');
  fillSelect('tool', r.tools, 'All tools');
  fillSelect('project', r.projects, 'All projects');
  fillSelect('model', r.models, 'All models');
  fillSelect('session', r.sessions, 'All sessions');

  $('#billTitle').textContent = title;
  $('#billCost').textContent = money(r.total.cost);
  $('#billSub').textContent = `${r.total.requests.toLocaleString()} requests · ${tok(inTok(r.total))} in · ${tok(r.total.output)} out`;

  $('#billChips').innerHTML = Object.keys(LABELS)
    .filter(k => F[k])
    .map(k => {
      const o = (r[LISTS[k]] || []).find(x => x.key === F[k]);
      return `<span class="chip filter-chip" data-clear="${k}">${LABELS[k]}: <b>${esc(o ? o.label : F[k])}</b><i>✕</i></span>`;
    })
    .join('');

  const days = r.days.length > 62 ? r.days.slice(-62) : r.days;
  bars($('#billDays'), days.map(d => d.cost), { highlightLast: false, title: (i, v) => `${days[i].weekday} ${days[i].date}: ${money(v)}` });
  const step = Math.max(1, Math.ceil(days.length / 6));
  $('#billAxis').innerHTML = days.filter((_, i) => i % step === 0).map(d => `<span>${d.date.slice(5)}</span>`).join('');
  $('#billDaysSub').textContent = days.length ? `${days.length} day${days.length === 1 ? '' : 's'} · avg ${money(r.total.cost / days.length)}/day` : '';

  const canOpen = ['project', 'session'].includes(F.group);
  $('#billRows').innerHTML = r.rows.length
    ? r.rows.map(x => `
      <div class="brow" data-key="${esc(x.key)}" title="Click to drill down">
        <div class="b-main">
          <div class="b-name">${esc(x.label)}</div>
          <div class="b-meta">${esc(x.sub)} · ${x.requests.toLocaleString()} req · ${tok(inTok(x))} in · ${tok(x.output)} out</div>
          <div class="b-share"><i style="width:${(x.share * 100).toFixed(1)}%"></i></div>
        </div>
        <div class="b-right"><b>${money(x.cost)}</b><span>${(x.share * 100).toFixed(1)}%</span></div>
        ${canOpen && x.dir ? `<button class="ico" data-open="${esc(x.dir)}" title="Open ${esc(x.dir)} in your editor">↗</button>` : ''}
      </div>`).join('')
    : '<div class="empty">No usage for these filters</div>';
  if (!$('#billFoot').dataset.sticky) $('#billFoot').textContent = `History loaded: ${r.history_days} days (Settings → System)`;
}

function setFoot(html) {
  const f = $('#billFoot');
  f.innerHTML = html;
  f.dataset.sticky = '1';
  clearTimeout(setFoot.t);
  setFoot.t = setTimeout(() => delete f.dataset.sticky, 15000);
}

export function initBill() {
  $('#billPeriod').addEventListener('click', e => {
    const b = e.target.closest('[data-p]');
    if (!b) return;
    F.period = b.dataset.p;
    if (F.period === 'custom' && !F.from) {
      const d = new Date();
      F.to = iso(d);
      F.from = iso(new Date(d.getFullYear(), d.getMonth(), 1));
      $('#billFrom').value = F.from;
      $('#billTo').value = F.to;
    }
    refreshBill();
  });
  $('#billFrom').addEventListener('change', e => { F.from = e.target.value; refreshBill(); });
  $('#billTo').addEventListener('change', e => { F.to = e.target.value; refreshBill(); });
  document.querySelectorAll('[data-bf]').forEach(sel =>
    sel.addEventListener('change', () => {
      F[sel.dataset.bf] = sel.value;
      if (sel.dataset.bf === 'project') F.session = '';
      refreshBill();
    })
  );
  $('#billHidden').addEventListener('change', e => { F.include_hidden = e.target.checked; refreshBill(); });
  $('#billReset').addEventListener('click', () => {
    Object.assign(F, { provider: '', tool: '', project: '', session: '', model: '' });
    refreshBill();
  });
  $('#billGroup').addEventListener('click', e => {
    const b = e.target.closest('[data-g]');
    if (!b) return;
    F.group = b.dataset.g;
    refreshBill();
  });
  $('#billChips').addEventListener('click', e => {
    const c = e.target.closest('[data-clear]');
    if (!c) return;
    F[c.dataset.clear] = '';
    refreshBill();
  });
  $('#billRows').addEventListener('click', e => {
    const o = e.target.closest('[data-open]');
    if (o) {
      e.stopPropagation();
      invoke('open_in_editor', { path: o.dataset.open }).catch(err => setFoot(esc(String(err))));
      return;
    }
    const row = e.target.closest('[data-key]');
    if (!row) return;
    const key = row.dataset.key;
    if (F.group === 'day') {
      F.period = 'custom';
      F.from = F.to = key;
      $('#billFrom').value = $('#billTo').value = key;
      F.group = 'session';
    } else {
      const [field, next] = DRILL[F.group] || DRILL.project;
      F[field] = key;
      F.group = next;
    }
    refreshBill();
  });
  $('#billExport').addEventListener('click', async () => {
    try {
      const path = await invoke('export_bill', { filter: filter() });
      setFoot(`Saved ${esc(path)} · <a id="billReveal">Show in folder</a>`);
      $('#billReveal').onclick = () => invoke('reveal_path', { path });
    } catch (err) {
      setFoot('Export failed: ' + esc(String(err)));
    }
  });
}
