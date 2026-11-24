/**
 * `<ascii-monolith src="/monolith.json">`: the codehead mascot, a monolith ray marched in
 * braille. The stone comes from the Rust lab (examples/lab, `just lab monolith-frames 6 64 34 21
 * > site/public/monolith.json`): per eye pattern, 1 frame per turn step with every eye off (its
 * sockets), glyphs + 1 tone digit per cell, and per eye dot the cells it lights. The eyes talk
 * live, as the agents' avatars do in codehead. The stone turns towards the pointer.
 * Keys: `s` status, `e` eye pattern (shown below it). Reduced motion: the middle frame, all eyes
 * lit, still.
 */

interface Frame {
  c: string;
  t: string;
  /** Per eye dot: [cell index, glyph] it lights. */
  e: [number, string][][];
}

interface Pattern {
  name: string;
  talk: string;
  dots: [number, number][];
  frames: Frame[];
}

interface Data {
  cols: number;
  rows: number;
  body: string;
  patterns: Pattern[];
}

const STATUSES = ['working', 'thinking', 'idle', 'blocked', 'done'] as const;
/** The pattern at the start: the cross of 4 rows (regi-008). */
const START_PATTERN = 3;

/** The dot bits of a braille glyph (a blank or space: 0), -1 for any other glyph. */
function bits(ch: string): number {
  if (ch === ' ') return 0;
  const c = ch.codePointAt(0)! - 0x2800;
  return c >= 0 && c < 256 ? c : -1;
}

const esc = (ch: string) => (ch === '<' ? '&lt;' : ch === '&' ? '&amp;' : ch === '>' ? '&gt;' : ch);

/** 0..1 from 2 integers: the chatter's random pick. */
function hash(a: number, b: number): number {
  let x = (a * 374761393 + b * 668265263) | 0;
  x = Math.imul(x ^ (x >>> 13), 1274126177);
  return ((x ^ (x >>> 16)) >>> 0) / 4294967295;
}

/** Per talk: [slot s, blink chance per slot, blink s] (regi.rs `blinks`). */
const TEMPO: Record<string, [number, number, number]> = {
  sweep: [0.55, 0.16, 0.11],
  chatter: [0.38, 0.14, 0.08],
  'center out': [0.6, 0.15, 0.12],
  chase: [0.45, 0.16, 0.09],
  morse: [0.5, 0.2, 0.07],
  rows: [0.7, 0.16, 0.13],
  halves: [0.48, 0.15, 0.1],
  stutter: [0.42, 0.22, 0.06],
};

/** Working: dot K off at time T. Each dot blinks on its own (its own slot length and phase, 1
 * short blink per slot by chance): the pattern stays in view, single dots wink. regi.rs `blinks`. */
function blinks(talk: string, k: number, t: number): boolean {
  const [slot, chance, dur] = TEMPO[talk] ?? TEMPO.sweep!;
  return blink(Object.keys(TEMPO).indexOf(talk) + 1, k, slot, chance, dur, t);
}

/** Dot K of SEED off at time T: per slot (SLOT s, ±30 % per dot, own phase) CHANCE of 1 blink of DUR s. */
function blink(seed: number, k: number, slot: number, chance: number, dur: number, t: number): boolean {
  const len = slot * (0.7 + 0.6 * hash(seed, k));
  const u = t + hash(seed + 101, k) * len;
  const j = Math.floor(u / len);
  const at = u - j * len;
  if (hash(seed * 7 + k, j * 3 + 2) >= chance) return false;
  const start = hash(seed * 7 + k, j * 3 + 3) * Math.max(0, len - dur);
  return at >= start && at < start + dur;
}

/** Thinking: as `blinks`, slower (longer slots, longer blinks). regi.rs `blinks_slow`. */
function blinksSlow(talk: string, k: number, t: number): boolean {
  const seed = Object.keys(TEMPO).indexOf(talk) + 41;
  return blink(seed, k, 1.5, 0.3, 0.26, t);
}

/** Dot K of P lit at time T (s), status S: regi.rs `lit` (blink style 1). */
function lit(p: Pattern, s: number, t: number, k: number): boolean {
  const d = p.dots;
  const [x] = d[k]!;
  const center = x === 0 && d[k]![1] === 0;
  const f = (rate: number) => Math.floor(t * rate);
  switch (STATUSES[s]) {
    case 'working':
      return !blinks(p.talk, k, t);
    case 'thinking':
      return !blinksSlow(p.talk, k, t);
    case 'idle': {
      const u = t % 3.3;
      return !(u < 0.1 || (u >= 0.22 && u < 0.32));
    }
    case 'blocked':
      return center || (f(4) % 2 === 0) === x <= 0;
    default: {
      const hasCenter = d.some(([a, b]) => a === 0 && b === 0);
      const mid = d.some(([a]) => a === 0);
      return center || (!hasCenter && (mid ? x === 0 : k === 0));
    }
  }
}

export class AsciiMonolith extends HTMLElement {
  #pre?: HTMLPreElement;
  /** The eye dots alone, over the stone: only the dots an eye lights turn lavender (the accent), 0 glow. */
  #eyes?: HTMLPreElement;
  #label?: HTMLParagraphElement;
  #data?: Data;
  #pattern = START_PATTERN;
  #status = 0;
  #pos = 0.5;
  #target = 0.5;
  #raf = 0;
  #visible = true;
  #still = false;
  #io?: IntersectionObserver;
  #last = '';

  async connectedCallback() {
    this.#pre = document.createElement('pre');
    this.#pre.setAttribute('aria-hidden', 'true');
    this.#eyes = document.createElement('pre');
    this.#eyes.className = 'eyes';
    this.#eyes.setAttribute('aria-hidden', 'true');
    const stack = document.createElement('div');
    stack.className = 'stack';
    stack.append(this.#pre, this.#eyes);
    this.#label = document.createElement('p');
    this.#label.className = 'label monolith-keys';
    this.append(stack, this.#label);
    try {
      this.#data = await (await fetch(this.getAttribute('src') || '/monolith.json')).json();
    } catch {
      return;
    }
    this.#pattern = Math.min(START_PATTERN, this.#data!.patterns.length - 1);
    this.#still = matchMedia('(prefers-reduced-motion: reduce)').matches;
    this.#draw(0);
    window.addEventListener('keydown', this.#onKey);
    if (this.#still) return;
    window.addEventListener('pointermove', this.#onPointer, { passive: true });
    this.#io = new IntersectionObserver(([e]) => {
      this.#visible = !!e?.isIntersecting;
      if (this.#visible) this.#loop();
    });
    this.#io.observe(this);
  }

  disconnectedCallback() {
    cancelAnimationFrame(this.#raf);
    window.removeEventListener('pointermove', this.#onPointer);
    window.removeEventListener('keydown', this.#onKey);
    this.#io?.disconnect();
  }

  #onKey = (e: KeyboardEvent) => {
    if (e.ctrlKey || e.metaKey || e.altKey || !this.#data) return;
    const el = e.target as HTMLElement | null;
    if (el && (el.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName))) return;
    if (e.key === 's') this.#status = (this.#status + 1) % STATUSES.length;
    else if (e.key === 'e') this.#pattern = (this.#pattern + 1) % this.#data.patterns.length;
    else return;
    this.#last = '';
    this.#draw(performance.now() / 1000);
  };

  #onPointer = (e: PointerEvent) => {
    const r = this.getBoundingClientRect();
    const dx = (e.clientX - (r.left + r.width / 2)) / window.innerWidth;
    this.#target = Math.min(1, Math.max(0, 0.5 + dx * 1.4));
  };

  #loop = () => {
    if (!this.#visible) return;
    this.#raf = requestAnimationFrame(this.#loop);
    const sway = 0.06 * Math.sin(performance.now() / 2500);
    this.#pos += (this.#target + sway - this.#pos) * 0.06;
    this.#draw(performance.now() / 1000);
  };

  /** The frame at the turn in view, the lit dots at time T. */
  #draw(t: number) {
    const p = this.#data!.patterns[this.#pattern]!;
    const i = Math.round(Math.min(1, Math.max(0, this.#pos)) * (p.frames.length - 1));
    const f = p.frames[i]!;
    const on = p.dots.map((_, k) => this.#still || lit(p, this.#status, t, k));
    const key = `${this.#pattern}:${i}:${on.join('')}:${this.#status}`;
    if (key === this.#last) return;
    this.#last = key;
    const chars = [...f.c];
    const tones = [...f.t];
    // the lit dots over the sockets, split per braille dot: dots in the socket glyph stay stone,
    // the dots the eye adds go to the overlay, tone 6 (7: amber, blocked; 8: dim, done)
    const eye = this.#status === 3 ? '7' : this.#status === 4 ? '8' : '6';
    const over = new Map<number, string>();
    // cell index → string index: the rows are joined by \n
    const cols = this.#data!.cols;
    on.forEach((o, k) => {
      if (!o) return;
      for (const [cell, ch] of f.e[k] ?? []) {
        const at = cell + Math.floor(cell / cols);
        const a = bits(chars[at]!);
        const b = bits(ch);
        if (a < 0 || b < 0) {
          // not braille: the whole glyph is eye
          chars[at] = ch;
          tones[at] = eye;
          continue;
        }
        chars[at] = String.fromCodePoint(0x2800 + (a & b));
        if (b & ~a) over.set(at, String.fromCodePoint(0x2800 + (b & ~a)));
      }
    });
    let out = '';
    let run = ' ';
    for (let j = 0; j < chars.length; j++) {
      const ch = chars[j]!;
      const tn = tones[j] ?? ' ';
      if (ch === '\n') {
        if (run !== ' ') out += '</b>';
        run = ' ';
        out += '\n';
        continue;
      }
      if (tn !== run) {
        if (run !== ' ') out += '</b>';
        if (tn !== ' ') out += `<b class="s${tn}">`;
        run = tn;
      }
      out += esc(ch);
    }
    if (run !== ' ') out += '</b>';
    this.#pre!.innerHTML = out;
    // the overlay: the same glyphs, transparent (same widths), the eye dots in their tone
    let eyes = '';
    for (let j = 0; j < chars.length; j++) {
      const e = over.get(j);
      eyes += e ? `<b class="s${eye}">${e}</b>` : esc(chars[j]!);
    }
    this.#eyes!.innerHTML = eyes;
    // the keys hint: shown on hover (landing.css), the status it acts out
    this.#label!.textContent = `${STATUSES[this.#status]} · s: status · e: eyes`;
  }
}

export function defineAsciiMonolith(): void {
  if (!customElements.get('ascii-monolith')) customElements.define('ascii-monolith', AsciiMonolith);
}
