// Captures README screenshots and GIF frames from a running Tokeye over the WebView2 DevTools protocol.
//
// 1. Start Tokeye with demo data (see scripts/demo_data.py) and remote debugging:
//      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333 tokeye.exe
// 2. node scripts/capture.mjs <out_dir> [shots|gif-notch|gif-eyes]
import fs from 'node:fs';
import path from 'node:path';

const OUT = path.resolve(process.argv[2] || 'docs/images');
const MODE = process.argv[3] || 'shots';
fs.mkdirSync(OUT, { recursive: true });
const sleep = ms => new Promise(r => setTimeout(r, ms));

async function connect(target) {
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise(r => (ws.onopen = r));
  let id = 0;
  const pending = new Map();
  const listeners = [];
  ws.onmessage = m => {
    const d = JSON.parse(m.data);
    if (d.id && pending.has(d.id)) {
      const { res, rej } = pending.get(d.id);
      pending.delete(d.id);
      d.error ? rej(new Error(d.error.message)) : res(d.result);
    } else if (d.method) listeners.forEach(l => l(d));
  };
  const send = (method, params = {}) => new Promise((res, rej) => { const i = ++id; pending.set(i, { res, rej }); ws.send(JSON.stringify({ id: i, method, params })); });
  const ev = async expr => {
    const r = await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text);
    return r.result.value;
  };
  return { send, ev, on: f => listeners.push(f), close: () => ws.close() };
}

async function pages() {
  const targets = (await (await fetch('http://127.0.0.1:9333/json')).json()).filter(t => t.type === 'page');
  const out = {};
  for (const t of targets) {
    const p = await connect(t);
    const label = await p.ev('window.__TAURI__.window.getCurrentWindow().label');
    out[label] = p;
  }
  return out;
}

async function shot(p, file, { clip, width, height, scale = 2 } = {}) {
  const size = await p.ev('({ w: innerWidth, h: innerHeight })');
  await p.send('Emulation.setDefaultBackgroundColorOverride', { color: { r: 0, g: 0, b: 0, a: 0 } });
  await p.send('Emulation.setDeviceMetricsOverride', { width: width || size.w, height: height || size.h, deviceScaleFactor: scale, mobile: false });
  await sleep(350);
  const params = { format: 'png' };
  if (clip) params.clip = { ...clip, scale: 1 };
  const r = await p.send('Page.captureScreenshot', params);
  fs.writeFileSync(path.join(OUT, file), Buffer.from(r.data, 'base64'));
  await p.send('Emulation.clearDeviceMetricsOverride');
  console.log('saved', file);
}

const rect = (p, sel, pad = 0) => p.ev(`(() => { const r = document.querySelector(${JSON.stringify(sel)}).getBoundingClientRect(); return { x: Math.max(0, r.left - ${pad}), y: Math.max(0, r.top - ${pad}), width: r.width + ${pad} * 2, height: r.height + ${pad} * 2 }; })()`);

async function dashboardShots(P) {
  const d = P.dashboard;
  await d.ev(`window.__TAURI__.core.invoke('show_dashboard_cmd', { tab: 'overview' })`);
  await sleep(1200);
  const tall = async (tab, file) => {
    await d.ev(`document.querySelector('[data-tab="${tab}"]').click()`);
    await sleep(900);
    // Local folder paths (demo data dirs) shouldn't appear in published screenshots.
    await d.ev(`document.querySelectorAll('[data-path], [data-k="claude_dir"]').forEach(i => (i.value = ''))`);
    const h = await d.ev(`(() => { const m = document.querySelector('main'); return 44 + 46 + m.scrollHeight + 4; })()`);
    await shot(d, file, { width: 440, height: Math.min(h, 2600) });
  };
  await tall('overview', 'dashboard-overview.png');
  await tall('bill', 'dashboard-bill.png');
  await tall('sessions', 'dashboard-sessions.png');
  await tall('models', 'dashboard-models.png');
  await tall('settings', 'dashboard-settings.png');
  // Light theme overview
  await d.ev(`(async () => { const s = await window.__TAURI__.core.invoke('get_settings'); await window.__TAURI__.core.invoke('save_settings', { settings: { ...s, theme: 'light' } }); })()`);
  await sleep(900);
  await tall('overview', 'dashboard-overview-light.png');
  await d.ev(`(async () => { const s = await window.__TAURI__.core.invoke('get_settings'); await window.__TAURI__.core.invoke('save_settings', { settings: { ...s, theme: 'dark' } }); })()`);
  await sleep(600);
  await d.ev(`document.querySelector('[data-tab="overview"]').click()`);
}

async function surfaceShots(P) {
  const n = P.notch;
  await shot(n, 'notch-collapsed.png', { clip: await rect(n, '#pill', 18) });
  await n.ev(`document.querySelector('#pill').classList.add('open')`);
  await sleep(900);
  await shot(n, 'notch-expanded.png', { clip: await rect(n, '#pill', 22) });
  await n.ev(`(() => { const a = document.querySelector('#alert'); a.textContent = 'Daily limit reached — You\\'ve used $102.40 of your $100.00 daily limit.'; document.querySelector('#pill').classList.add('alerting'); })()`);
  await sleep(700);
  await shot(n, 'notch-alert.png', { clip: await rect(n, '#pill', 22) });
  await n.ev(`document.querySelector('#pill').classList.remove('alerting', 'open')`);
  await shot(P.floating, 'bubble.png');
  await shot(P.widget, 'widget.png');
  await shot(P.taskbar, 'taskbar.png');
  await P.dashboard.ev(`window.__TAURI__.core.invoke('open_menu_cmd')`);
  await sleep(1200);
  await shot(P.menu, 'menu.png');
  await P.menu.ev(`window.__TAURI__.core.invoke('hide_window', { label: 'menu' })`);
}

async function moodSheet(P) {
  const d = P.dashboard;
  await d.ev(`(async () => {
    const { Eyes } = await import('/js/eyes.js');
    const moods = [['calm', 'Calm'], ['happy', 'Happy'], ['working', 'Working'], ['wink', 'Wink'], ['love', 'Love'], ['worried', 'Near a limit'], ['dizzy', 'Over a limit'], ['sleepy', 'Sleepy'], ['shocked', 'Big spend']];
    const sheet = document.createElement('div');
    sheet.id = 'sheet';
    sheet.style.cssText = 'position:fixed;inset:0;z-index:99;background:#0d0d11;display:grid;grid-template-columns:repeat(3,1fr);gap:10px;padding:18px;';
    document.body.appendChild(sheet);
    const st = document.createElement('style');
    st.textContent = '#sheet .eyes.blink .blinker{transform:none!important} #sheet .cell{background:#000;border-radius:8px;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:10px;color:#a3a3ad;font:600 12px var(--font)} #sheet .cell .eyes{width:110px;height:56px}';
    document.head.appendChild(st);
    for (const [m, label] of moods) {
      const cell = document.createElement('div'); cell.className = 'cell';
      const host = document.createElement('div'); cell.appendChild(host);
      const cap = document.createElement('span'); cap.textContent = label; cell.appendChild(cap);
      sheet.appendChild(cell);
      const e = new Eyes(host, { interactive: false });
      if (m === 'working') e.setState({ working: true }); else if (m !== 'calm') e.react(m, 1e9);
    }
  })()`);
  await sleep(1500);
  await shot(d, 'companion-moods.png', { width: 440, height: 400 });
  await d.ev(`document.getElementById('sheet').remove()`);
}

// Screencast frames (PNG with transparency) with timestamps, for assembling GIFs.
async function record(p, dir, ms, actions) {
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  const frames = [];
  p.on(d => {
    if (d.method !== 'Page.screencastFrame') return;
    const f = path.join(dir, String(frames.length).padStart(4, '0') + '.png');
    fs.writeFileSync(f, Buffer.from(d.params.data, 'base64'));
    frames.push({ file: path.basename(f), t: d.params.metadata.timestamp });
    p.send('Page.screencastFrameAck', { sessionId: d.params.sessionId }).catch(() => {});
  });
  await p.send('Emulation.setDefaultBackgroundColorOverride', { color: { r: 0, g: 0, b: 0, a: 0 } });
  await p.send('Page.startScreencast', { format: 'png', everyNthFrame: 1 });
  const t0 = Date.now();
  for (const [at, fn] of actions) {
    const wait = at - (Date.now() - t0);
    if (wait > 0) await sleep(wait);
    await fn();
  }
  const rest = ms - (Date.now() - t0);
  if (rest > 0) await sleep(rest);
  await p.send('Page.stopScreencast');
  fs.writeFileSync(path.join(dir, 'frames.json'), JSON.stringify(frames));
  console.log('recorded', frames.length, 'frames into', dir);
}

async function gifNotch(P) {
  const n = P.notch;
  const pos = await n.ev(`window.__TAURI__.window.getCurrentWindow().innerPosition().then(p => ({ x: p.x, y: p.y }))`);
  const look = (dx, dy) => () => n.ev(`window.__TAURI__.event.emit('pointer', [${pos.x + 260 + dx}, ${pos.y + 18 + dy}])`);
  const pill = () => n.ev(`document.querySelector('#pill')`);
  await record(n, path.join(OUT, '_frames-notch'), 9000, [
    [100, look(-600, 300)],
    [700, look(600, 300)],
    [1300, look(0, 500)],
    [1800, () => n.ev(`document.querySelector('#pill').classList.add('open')`)],
    [2400, look(140, 60)],
    [5200, () => n.ev(`document.querySelector('#eyes').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`)],
    [7600, () => n.ev(`document.querySelector('#pill').classList.remove('open')`)],
    [8200, look(-300, 200)],
  ]);
  void pill;
}

async function gifEyes(P) {
  const d = P.dashboard;
  await d.ev(`(async () => {
    const { Eyes } = await import('/js/eyes.js');
    const hide = document.createElement('style');
    hide.id = 'stageHide';
    hide.textContent = 'html, body { background: transparent !important; } body > :not(#stage) { visibility: hidden !important; }';
    document.head.appendChild(hide);
    const stage = document.createElement('div');
    stage.id = 'stage';
    stage.style.cssText = 'position:fixed;inset:0;z-index:99;background:transparent;display:flex;align-items:center;justify-content:center;';
    const box = document.createElement('div');
    box.style.cssText = 'width:300px;height:150px;background:#000;border-radius:14px;display:flex;align-items:center;justify-content:center;';
    const host = document.createElement('div'); host.style.cssText = 'width:200px;height:100px';
    box.appendChild(host); stage.appendChild(box); document.body.appendChild(stage);
    window.__eyes = new Eyes(host, { interactive: false });
  })()`);
  await sleep(600);
  const m = (mood, ms = 1300) => () => d.ev(`window.__eyes.react('${mood}', ${ms})`);
  await record(d, path.join(OUT, '_frames-eyes'), 10500, [
    [300, () => d.ev(`window.__eyes.t.x = -0.9; window.__eyes.t.y = 0.2`)],
    [900, () => d.ev(`window.__eyes.t.x = 0.9; window.__eyes.t.y = -0.3`)],
    [1500, () => d.ev(`window.__eyes.t.x = 0; window.__eyes.t.y = 0; window.__eyes.blink()`)],
    [2000, m('happy')],
    [3300, m('wink', 900)],
    [4300, () => d.ev(`window.__eyes.react('love', 1600); window.__eyes.burst(['❤','♥'], 6)`)],
    [6000, m('worried', 1300)],
    [7400, m('dizzy', 1500)],
    [9000, m('sleepy', 1400)],
  ]);
  await d.ev(`document.getElementById('stage').remove(); document.getElementById('stageHide').remove()`);
}

const P = await pages();
if (MODE === 'settings') {
  await P.dashboard.ev(`window.__TAURI__.core.invoke('show_dashboard_cmd', { tab: 'settings' })`);
  await sleep(1200);
  await P.dashboard.ev(`document.querySelectorAll('[data-path], [data-k="claude_dir"]').forEach(i => (i.value = ''))`);
  const h = await P.dashboard.ev(`44 + 46 + document.querySelector('main').scrollHeight + 4`);
  await shot(P.dashboard, 'dashboard-settings.png', { width: 440, height: Math.min(h, 2600) });
} else if (MODE === 'shots') {
  await dashboardShots(P);
  await surfaceShots(P);
  await moodSheet(P);
} else if (MODE === 'gif-notch') {
  await gifNotch(P);
} else if (MODE === 'gif-eyes') {
  await gifEyes(P);
}
Object.values(P).forEach(p => p.close());
process.exit(0);
