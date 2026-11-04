import { invoke, listen, $, state, boot, money, tok, inTok, saveSettings, moodFeeder, surface } from './common.js';
import { Eyes } from './eyes.js';

const eyes = new Eyes($('#eyes'));
const feed = moodFeeder(eyes);

function render(st) {
  if (!st || !st.ready) return;
  $('#today').textContent = money(st.today.cost);
  $('#sub').textContent = `today · ${tok(inTok(st.today))} in · ${tok(st.today.output)} out · month ${money(st.month.cost)}`;
  feed(st);
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
});

invoke('platform').then(os => {
  $('#autoLabel').textContent = os === 'windows' ? 'Start with Windows' : 'Start at login';
});

boot({ onStats: render, onSettings: fill }).then(() => {
  // Fit the window to the menu's content.
  const h = Math.ceil($('#box').getBoundingClientRect().height + 16);
  invoke('resize_self', { width: 280, height: h });
});
