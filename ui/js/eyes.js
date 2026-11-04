// The Tokeye companion: two expressive eyes that blink, follow the cursor and react to your spending.
const svg = id => `
<svg viewBox="0 0 100 50" aria-hidden="true">
  <defs>${['L', 'R'].map(s => `
    <mask id="m${s}${id}" class="${s}" maskUnits="userSpaceOnUse" x="-40" y="-40" width="80" height="80">
      <rect x="-30" y="-30" width="60" height="60" fill="#fff"/>
      <rect class="lid top" x="-26" y="-46" width="52" height="26" fill="#000"/>
      <ellipse class="cheek" cx="0" cy="36" rx="24" ry="17" fill="#000"/>
    </mask>`).join('')}
  </defs>
  <g class="face">${[['L', 30], ['R', 70]].map(([s, x]) => `
    <g class="eye ${s}" transform="translate(${x} 25)">
      <g class="look">
        <g class="blinker">
          <rect class="ball" x="-12" y="-15" width="24" height="30" rx="10" mask="url(#m${s}${id})"/>
          <path class="heart" d="M0 9C-13 0-13-11-6-11C-2.5-11 0-8.5 0-6.5C0-8.5 2.5-11 6-11C13-11 13 0 0 9Z"/>
          <path class="xx" d="M-8-8L8 8M8-8L-8 8"/>
        </g>
      </g>
    </g>`).join('')}
    <g class="dots"><circle cx="42" cy="49" r="2.2"/><circle cx="50" cy="49" r="2.2"/><circle cx="58" cy="49" r="2.2"/></g>
    <path class="sweat" d="M90 4C87 9 87 12 90 12C93 12 93 9 90 4Z"/>
    <text class="zz" x="86" y="6">z</text>
  </g>
</svg>`;

const clamp = (v, a = -1, b = 1) => Math.max(a, Math.min(b, v));
const rand = (a, b) => a + Math.random() * (b - a);

// Eye shapes: [x, y, width, height, corner radius] of each eye, centered on 0,0.
export const SHAPES = {
  rounded: [-12, -15, 24, 30, 10],
  round: [-13, -13, 26, 26, 13],
  tall: [-9, -17, 18, 34, 9],
  square: [-12, -12, 24, 24, 4],
  wide: [-15, -10, 30, 20, 9],
  dot: [-7, -7, 14, 14, 7],
};

// How lively the companion is.
const PERSONALITY = {
  calm: { glance: 0.25, happy: 0.0, blink: 1.5, sparkle: 0 },
  normal: { glance: 0.55, happy: 0.05, blink: 1, sparkle: 0.02 },
  playful: { glance: 0.85, happy: 0.14, blink: 0.75, sparkle: 0.1 },
};

export class Eyes {
  constructor(host, { interactive = true } = {}) {
    this.host = host;
    this.id = Math.random().toString(36).slice(2, 8);
    host.classList.add('eyes');
    host.innerHTML = svg(this.id);
    this.looks = [...host.querySelectorAll('.look')];
    this.g = { x: 0, y: 0 };
    this.t = { x: 0, y: 0 };
    this.base = 'calm';
    this.temp = null;
    this.cur = null;
    this.level = 0;
    this.working = false;
    this.idleMins = 0;
    this.lastMove = 0;
    this.near = false;
    this.follow = true;
    this.p = PERSONALITY.normal;
    this.pet = { dir: 0, flips: 0, t0: 0, lastX: 0 };
    this._apply();
    this._frame = this._frame.bind(this);
    requestAnimationFrame(this._frame);
    this._blinkLoop();
    setInterval(() => this._idle(), 1400);
    if (interactive) this._bind();
  }

  // ----- public -----
  /** Applies shape / personality / follow-cursor from settings. */
  configure(s) {
    const shape = SHAPES[s.eye_shape] || SHAPES.rounded;
    if (shape !== this._shape) {
      this._shape = shape;
      const [x, y, w, h, rx] = shape;
      this.host.querySelectorAll('.ball').forEach(b => {
        b.setAttribute('x', x);
        b.setAttribute('y', y);
        b.setAttribute('width', w);
        b.setAttribute('height', h);
        b.setAttribute('rx', rx);
      });
    }
    this.p = PERSONALITY[s.personality] || PERSONALITY.normal;
    this.follow = s.follow_cursor !== false;
  }

  lookAt(nx, ny, dist = 9999) {
    if (!this.follow) return;
    this.t.x = clamp(nx);
    this.t.y = clamp(ny);
    const now = performance.now();
    if (now - this.lastMove > 20000 && this.base === 'sleepy' && dist < 400) this.react('shocked', 700);
    this.lastMove = now;
    this.near = dist < 140;
  }

  setState({ level = 0, working = false, idleMins = 0 } = {}) {
    this.level = level;
    this.working = working;
    this.idleMins = idleMins;
    this._recompute();
  }

  react(mood, ms = 1200) {
    this.temp = mood;
    this._apply();
    clearTimeout(this._tt);
    this._tt = setTimeout(() => {
      this.temp = null;
      this._apply();
    }, ms);
  }

  blink() {
    this.host.classList.add('blink');
    setTimeout(() => this.host.classList.remove('blink'), 115);
  }

  hop() {
    this._pulse('hop', 520);
  }

  burst(chars, n = 4) {
    for (let i = 0; i < n; i++) {
      setTimeout(() => {
        const p = document.createElement('span');
        p.className = 'particle';
        p.textContent = chars[i % chars.length];
        p.style.left = rand(20, 80) + '%';
        p.style.top = rand(0, 40) + '%';
        p.style.setProperty('--dx', rand(-18, 18).toFixed(0) + 'px');
        p.style.fontSize = rand(9, 14).toFixed(0) + 'px';
        this.host.appendChild(p);
        setTimeout(() => p.remove(), 1400);
      }, i * 110);
    }
  }

  // ----- internals -----
  _recompute() {
    const now = performance.now();
    const pointerIdle = now - this.lastMove > 60000;
    let m = 'calm';
    if (this.level >= 2) m = 'dizzy';
    else if (this.level === 1) m = 'worried';
    else if (this.working) m = 'working';
    else if (this.idleMins > 20 && pointerIdle) m = 'sleepy';
    this.base = m;
    this._apply();
  }

  _apply() {
    const m = this.temp || this.base;
    if (m === this.cur) return;
    if (this.cur) this.host.classList.remove('mood-' + this.cur);
    this.host.classList.add('mood-' + m);
    this.cur = m;
  }

  _pulse(cls, ms) {
    this.host.classList.remove(cls);
    void this.host.getBoundingClientRect();
    this.host.classList.add(cls);
    clearTimeout(this['_p' + cls]);
    this['_p' + cls] = setTimeout(() => this.host.classList.remove(cls), ms);
  }

  _frame() {
    const k = 0.22;
    this.g.x += (this.t.x - this.g.x) * k;
    this.g.y += (this.t.y - this.g.y) * k;
    const m = this.cur;
    let gx = this.g.x, gy = this.g.y;
    if (m === 'working') {
      gx = 0.55 + Math.sin(performance.now() / 380) * 0.18;
      gy = 0.35;
    } else if (m === 'sleepy' || m === 'dizzy') {
      gx *= 0.2;
      gy = gy * 0.2 + 0.25;
    }
    const tr = `translate(${(gx * 7).toFixed(2)} ${(gy * 5).toFixed(2)})`;
    if (tr !== this._lastTr) {
      this.looks.forEach(l => l.setAttribute('transform', tr));
      this._lastTr = tr;
    }
    requestAnimationFrame(this._frame);
  }

  _blinkLoop() {
    setTimeout(() => {
      if (!['sleepy', 'love', 'dizzy', 'wink'].includes(this.cur) && !document.hidden) {
        this.blink();
        if (Math.random() < 0.2) setTimeout(() => this.blink(), 190);
      }
      this._blinkLoop();
    }, rand(1800, 5600) * this.p.blink);
  }

  _idle() {
    this._recompute();
    const now = performance.now();
    if (this.cur === 'sleepy') {
      if (Math.random() < 0.35) this.burst(['z', 'Z'], 1);
      return;
    }
    // When the cursor rests (or following is off), glance around on our own.
    if ((now - this.lastMove > 2500 || !this.follow) && this.cur !== 'working') {
      if (Math.random() < this.p.glance) {
        this.t.x = rand(-0.9, 0.9);
        this.t.y = rand(-0.6, 0.7);
      } else if (Math.random() < 0.3) {
        this.t.x = 0;
        this.t.y = 0;
      }
      if (Math.random() < this.p.happy && this.level === 0) this.react('happy', 900);
      if (Math.random() < this.p.sparkle && this.level === 0) {
        this.hop();
        this.burst(['✦', '✧'], 2);
      }
    }
  }

  _bind() {
    const h = this.host;
    h.addEventListener('mouseenter', () => {
      if (this.level < 2) this.react('happy', 900);
    });
    h.addEventListener('click', e => {
      e.stopPropagation();
      this._pulse('boop', 450);
      this.react(this.level >= 2 ? 'shocked' : 'wink', 750);
      this.burst(['✦', '✧'], 3);
    });
    h.addEventListener('dblclick', e => {
      e.stopPropagation();
      this.react('love', 2300);
      this.burst(['❤', '♥'], 6);
    });
    // "Petting": wiggle the mouse left and right over the eyes.
    h.addEventListener('mousemove', e => {
      const p = this.pet, now = performance.now();
      const dir = Math.sign(e.clientX - p.lastX);
      p.lastX = e.clientX;
      if (!dir) return;
      if (now - p.t0 > 1400) {
        p.flips = 0;
        p.t0 = now;
      }
      if (dir !== p.dir) {
        p.dir = dir;
        p.flips++;
      }
      if (p.flips >= 6) {
        p.flips = 0;
        p.t0 = now;
        this.react('giggle', 1200);
        this.burst(['❤', '✦'], 4);
      }
    });
  }
}
