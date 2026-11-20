/**
 * `<ascii-moai src="/moai.json">`: the codehead mascot, a moai ray marched in text. The frames
 * come from the Rust lab (examples/lab, `just lab frames edge 64 34 41 > site/public/moai.json`):
 * 1 frame per turn step, glyphs + 1 tone digit per cell (0-5 brightness, 6 coral eye). The head
 * turns towards the pointer and sways a little. Reduced motion: the middle frame, still.
 */

interface Frames {
  cols: number;
  rows: number;
  frames: { c: string; t: string }[];
}

const esc = (ch: string) => (ch === '<' ? '&lt;' : ch === '&' ? '&amp;' : ch === '>' ? '&gt;' : ch);

/** 1 frame as HTML: runs of the same tone in 1 <b class="sN">. */
function html(f: { c: string; t: string }): string {
  let out = '';
  let run = ' ';
  for (let i = 0; i < f.c.length; i++) {
    const ch = f.c[i]!;
    const t = f.t[i] ?? ' ';
    if (ch === '\n') {
      if (run !== ' ') out += '</b>';
      run = ' ';
      out += '\n';
      continue;
    }
    if (t !== run) {
      if (run !== ' ') out += '</b>';
      if (t !== ' ') out += `<b class="s${t}">`;
      run = t;
    }
    out += esc(ch);
  }
  if (run !== ' ') out += '</b>';
  return out;
}

export class AsciiMoai extends HTMLElement {
  #pre?: HTMLPreElement;
  #frames: string[] = [];
  #shown = -1;
  #pos = 0.5;
  #target = 0.5;
  #raf = 0;
  #visible = true;
  #io?: IntersectionObserver;

  async connectedCallback() {
    this.#pre = document.createElement('pre');
    this.#pre.setAttribute('aria-hidden', 'true');
    this.append(this.#pre);
    try {
      const data: Frames = await (await fetch(this.getAttribute('src') || '/moai.json')).json();
      this.#frames = data.frames.map(html);
    } catch {
      return;
    }
    this.#show(0.5);
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
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
    this.#io?.disconnect();
  }

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
    this.#show(this.#pos);
  };

  #show(p: number) {
    const i = Math.round(Math.min(1, Math.max(0, p)) * (this.#frames.length - 1));
    if (i === this.#shown || !this.#frames[i]) return;
    this.#shown = i;
    this.#pre!.innerHTML = this.#frames[i]!;
  }
}

export function defineAsciiMoai(): void {
  if (!customElements.get('ascii-moai')) customElements.define('ascii-moai', AsciiMoai);
}
