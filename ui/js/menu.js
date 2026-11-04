import { invoke, listen, $, state, boot, money, tok, inTok, saveSettings, moodFeeder, surface } from './common.js';
import { Eyes } from './eyes.js';

const eyes = new Eyes($('#eyes'));
const feed = moodFeeder(eyes);

let lastH = 0;
let fitTimer = 0;
function fit() {
  // While the window is hidden its viewport can be tiny, which makes every
  // label wrap and reports a bogus height. Wait until it has a real width.
  if (document.hidden || document.documentElement.clientWidth < 200) return;
  const box = $('#box');
  const first = box.getBoundingClientRect();
  const last = box.lastElementChild;
  if (!last) return;
  const h = Math.ceil(last.getBoundingClientRect().bottom - first.top + 16);
  if (h === lastH) return;
  lastH = h;
  invoke('resize_self', { width: 280, height: h });
}
function fitSoon() {
  clearTimeout(fitTimer);
  fitTimer = setTimeout(fit, 80);
}

function render(st) {
  if (!st || !st.ready) return;
  $('#today').textContent = money(st.today.cost);
  $('#sub').textContent = `today · ${tok(inTok(st.today))} in · ${tok(st.today.output)} out · month ${money(st.month.cost)}`;
  feed(st);
  fitSoon();
}

function fill(s) {
  document.querySelectorAll('[data-k]').forEach(el => (el.checked = !!s[el.dataset.k]));
  $('#limitK').textContent = s.daily_limit > 0 ? `${money(s.daily_limit)}/day` : 'off';
  $('#ghost').checked = surface('floating').ghost || surface('widget').ghost;
}

$('#ghost').addEventListener('change', e => {
  const ghost = e.target.checked;
  const all = { ...(state.settings.surfaces || {}) };
  for (const k of ['floating', 'widget']) all[k] = { ...surface(k), ghost };
  saveSettings({ surfaces: all });
});

document.querySelectorAll('[data-k]').forEach(el =>
  el.addEventListener('change', () => saveSettings({ [el.dataset.k]: el.checked }))
);

document.querySelectorAll('[data-act]').forEach(el =>
  el.addEventListener('click', () => {
    const a = el.dataset.act;
    if (a === 'quit') invoke('quit');
    else invoke('show_dashboard_cmd', { tab: a === 'dash' ? 'overview' : 'settings' });
  })
);

document.addEventListener('keydown', e => e.key === 'Escape' && invoke('hide_window', { label: 'menu' }));
document.addEventListener('contextmenu', e => e.preventDefault());
listen('menu-open', () => {
  eyes.react('happy', 900);
  eyes.hop();
  fitSoon();
});

invoke('platform').then(os => {
  $('#autoLabel').textContent = os === 'windows' ? 'Start with Windows' : 'Start at login';
});

window.addEventListener('resize', fitSoon);
document.addEventListener('visibilitychange', () => !document.hidden && fitSoon());
boot({ onStats: render, onSettings: fill }).then(fitSoon);
