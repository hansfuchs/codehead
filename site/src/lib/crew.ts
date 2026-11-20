/**
 * The crew: codehead's agent avatars on the website, from the real renderer (examples/lab,
 * `just lab avatar-frames 10 4 "regi:45" "regi:40" "regi:30 eyes=11 speed=0.5" "regi:61" >
 * site/public/crew.json`): per avatar, 4 s of frames per status and 1 spawn (the goo forming
 * the sprite). 1 scripted thread plays on a 24 s loop, the same clock for every element:
 *
 * - `<crew-stage src="/crew.json">`: 4 agent cards (avatar, name, harness, status), a `send`
 *   arrow between 2 cards, the last lines of the thread
 * - `<crew-strip src="/crew.json">`: the header cell: the avatars, small, and 1 status line
 *
 * Reduced motion: every agent idle, 0 animation.
 */

type Status = 'working' | 'thinking' | 'idle' | 'blocked' | 'done';

interface Avatar {
  id: string;
  name: string;
  status: Record<Status, string[][]>;
  spawn: string[][];
  at: [number, number];
}

interface Data {
  fps: number;
  w: number;
  h: number;
  sw: number;
  sh: number;
  avatars: Avatar[];
}

interface Agent {
  name: string;
  harness: string;
  color: string;
}

/** The crew, in crew.json's order. */
const AGENTS: Agent[] = [
  { name: 'EVA', harness: 'Claude Code · opus', color: 'var(--coral)' },
  { name: 'HAL', harness: 'Codex · gpt-5.5', color: '#8fb3d9' },
  { name: 'DMON', harness: 'Claude Code · sonnet', color: '#b9a0d9' },
  { name: 'ENOCH', harness: 'OpenCode · kimi-k2', color: '#9cc28a' },
];

/** 1 beat of the scripted thread: from T (s), the agents' states and the line it adds. */
interface Beat {
  t: number;
  /** Per agent: a status, 'spawn', or absent ('-': not on the thread). */
  s: (Status | 'spawn' | '-')[];
  line?: [string, string];
  /** A send: from → to (agent indexes). */
  send?: [number, number];
}

const LOOP = 24;
const BEATS: Beat[] = [
  { t: 0, s: ['spawn', '-', '-', '-'], line: ['you', 'stop the 410 retry storm. tests first. HAL reviews.'] },
  { t: 1.8, s: ['thinking', '-', '-', '-'], line: ['EVA', 'the cap checks the next delay alone. fixing the comparison.'] },
  { t: 4, s: ['working', '-', '-', '-'], line: ['EVA', '▸ edit retry.rs, run the retry tests · 3 calls · 7.4s'] },
  { t: 7, s: ['working', 'spawn', '-', '-'], line: ['EVA → HAL', 'review 4ff478f. block on correctness only.'], send: [0, 1] },
  { t: 8.8, s: ['idle', 'working', '-', '-'], line: ['HAL', '▸ show the commit, find attempt counters · 2 calls'] },
  { t: 11, s: ['idle', 'blocked', '-', '-'], line: ['HAL', 'waits on you: run the overflow test on attempt 40?  1 yes  2 no'] },
  { t: 13, s: ['idle', 'working', '-', '-'], line: ['you', '1'] },
  { t: 14.2, s: ['working', 'idle', '-', '-'], line: ['HAL → EVA', '1 blocker. retry.rs:11 pow(attempt) overflows from 32.'], send: [1, 0] },
  { t: 16.5, s: ['blocked', 'idle', '-', '-'], line: ['EVA', "You've hit your limit · resets 18:00"] },
  { t: 18, s: ['done', 'idle', '-', 'spawn'], line: ['you', '@ENOCH (replaces @EVA) take over: checked_pow, test for 40.'] },
  { t: 19.8, s: ['-', 'idle', '-', 'working'], line: ['ENOCH', '▸ edit retry.rs, run the retry tests · 6/6 pass'] },
  { t: 21.5, s: ['-', 'done', '-', 'done'], line: ['HAL', 'LGTM on 00c6252. approved.'], send: [3, 1] },
];

const LABEL: Record<string, string> = {
  working: 'working…',
  thinking: 'thinking…',
  idle: 'idle',
  blocked: 'waits on you',
  done: 'done',
  spawn: 'joins',
  '-': 'not on the thread',
};

let data: Promise<Data> | null = null;
function load(src: string): Promise<Data> {
  data ??= fetch(src).then((r) => r.json());
  return data;
}

const still = () => matchMedia('(prefers-reduced-motion: reduce)').matches;

/** The beat at loop time T, and the time since it began. */
function beatAt(t: number): [Beat, number, number] {
  let i = 0;
  for (let k = 0; k < BEATS.length; k++) if (BEATS[k]!.t <= t) i = k;
  return [BEATS[i]!, t - BEATS[i]!.t, i];
}

/** The 13x6 rows of avatar A at loop time T: its spawn, else its status frame in the middle. */
function canvas(d: Data, a: Avatar, st: Status | 'spawn' | '-', since: number, t: number): string[] {
  if (st === '-') return Array.from({ length: d.sh }, () => ' '.repeat(d.sw));
  if (st === 'spawn') {
    const f = Math.min(a.spawn.length - 1, Math.floor(since * d.fps));
    if (f < a.spawn.length - 1) return a.spawn[f]!;
    st = 'idle';
  }
  const frames = a.status[st];
  const rows = frames[Math.floor(t * d.fps) % frames.length]!;
  const [x, y] = a.at;
  const pad = ' '.repeat(x);
  const right = ' '.repeat(d.sw - x - d.w);
  const blank = ' '.repeat(d.sw);
  return Array.from({ length: d.sh }, (_, r) => (r >= y && r < y + d.h ? pad + rows[r - y] + right : blank));
}

const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;');

class CrewBase extends HTMLElement {
  protected d?: Data;
  #raf = 0;
  #visible = true;
  #io?: IntersectionObserver;
  protected still = false;

  async connectedCallback() {
    this.still = still();
    try {
      this.d = await load(this.getAttribute('src') || '/crew.json');
    } catch {
      return;
    }
    this.build();
    this.draw(this.still ? 9 : (performance.now() / 1000) % LOOP);
    if (this.still) return;
    this.#io = new IntersectionObserver(([e]) => {
      this.#visible = !!e?.isIntersecting;
      if (this.#visible) this.#loop();
    });
    this.#io.observe(this);
  }

  disconnectedCallback() {
    cancelAnimationFrame(this.#raf);
    this.#io?.disconnect();
  }

  #last = 0;
  #loop = () => {
    if (!this.#visible) return;
    this.#raf = requestAnimationFrame(this.#loop);
    const now = performance.now();
    // the avatars run at 10 fps in codehead: 0 reason to draw more often
    if (now - this.#last < 1000 / (this.d?.fps ?? 10)) return;
    this.#last = now;
    this.draw((now / 1000) % LOOP);
  };

  protected build() {}
  protected draw(_t: number) {}
}

/** The stage: 4 cards, the send arrow, the thread's last lines. */
class CrewStage extends CrewBase {
  #cards: { pre: HTMLPreElement; st: HTMLElement; el: HTMLElement }[] = [];
  #log?: HTMLOListElement;
  #beat = -1;

  protected build() {
    const d = this.d!;
    const grid = document.createElement('div');
    grid.className = 'crew-cards';
    d.avatars.forEach((_, i) => {
      const ag = AGENTS[i]!;
      const el = document.createElement('div');
      el.className = 'crew-card';
      el.style.setProperty('--agent', ag.color);
      const pre = document.createElement('pre');
      pre.setAttribute('aria-hidden', 'true');
      const meta = document.createElement('div');
      meta.className = 'crew-meta';
      meta.innerHTML = `<span class="crew-name">${ag.name}</span><span class="crew-harness">${ag.harness}</span>`;
      const st = document.createElement('span');
      st.className = 'crew-status';
      meta.append(st);
      el.append(pre, meta);
      grid.append(el);
      this.#cards.push({ pre, st, el });
    });
    this.#log = document.createElement('ol');
    this.#log.className = 'crew-log';
    this.#log.setAttribute('aria-live', 'off');
    this.append(grid, this.#log);
  }

  protected draw(t: number) {
    const d = this.d!;
    const [beat, since, bi] = beatAt(t);
    this.#cards.forEach((c, i) => {
      const s = beat.s[i] ?? '-';
      c.pre.textContent = canvas(d, d.avatars[i]!, s, since, t).join('\n');
      const shown = s === 'spawn' && since * d.fps >= d.avatars[i]!.spawn.length - 1 ? 'idle' : s;
      c.el.dataset.status = shown;
      c.el.classList.toggle('sending', !!beat.send && beat.send[0] === i && since < 1.6);
      c.el.classList.toggle('receiving', !!beat.send && beat.send[1] === i && since >= 0.5 && since < 1.8);
      c.st.textContent = LABEL[shown] ?? '';
    });
    if (bi === this.#beat) return;
    this.#beat = bi;
    // the thread: the last 4 lines up to this beat
    const lines = BEATS.slice(0, bi + 1)
      .filter((b) => b.line)
      .slice(-4);
    this.#log!.innerHTML = lines
      .map((b, k) => {
        const [who, text] = b.line!;
        const first = who.split(' ')[0]!;
        const ag = AGENTS.find((a) => a.name === first);
        const color = ag ? ag.color : 'var(--ink)';
        const fresh = k === lines.length - 1 ? ' class="fresh"' : '';
        return `<li${fresh}><span class="who" style="color:${color}">${esc(who)}</span><span class="what">${esc(text)}</span></li>`;
      })
      .join('');
  }
}

/** The header cell: the avatars on the thread, small, and 1 line of what happens. */
class CrewStrip extends CrewBase {
  #pres: HTMLPreElement[] = [];
  #line?: HTMLElement;

  protected build() {
    const d = this.d!;
    const faces = document.createElement('span');
    faces.className = 'strip-faces';
    faces.setAttribute('aria-hidden', 'true');
    d.avatars.forEach((_, i) => {
      const pre = document.createElement('pre');
      pre.style.color = AGENTS[i]!.color;
      faces.append(pre);
      this.#pres.push(pre);
    });
    this.#line = document.createElement('span');
    this.#line.className = 'strip-line truncate';
    this.append(faces, this.#line);
  }

  protected draw(t: number) {
    const d = this.d!;
    const [beat, since] = beatAt(t);
    this.#pres.forEach((pre, i) => {
      const s = beat.s[i] ?? '-';
      // the 7x4 sprite only (the spawn's 13x6 canvas is too wide for the bar)
      const rows = canvas(d, d.avatars[i]!, s, since, t).slice(d.avatars[i]!.at[1], d.avatars[i]!.at[1] + d.h);
      pre.textContent = rows.map((r) => r.slice(d.avatars[i]!.at[0], d.avatars[i]!.at[0] + d.w)).join('\n');
      pre.dataset.status = s === 'spawn' ? 'idle' : s;
      pre.hidden = s === '-';
    });
    const on = beat.s
      .map((s, i) => (s === '-' ? '' : `${AGENTS[i]!.name} ${LABEL[s === 'spawn' ? 'spawn' : s]}`))
      .filter(Boolean)
      .join(' · ');
    if (this.#line!.textContent !== on) this.#line!.textContent = on;
  }
}

export function defineCrew(): void {
  if (!customElements.get('crew-stage')) customElements.define('crew-stage', CrewStage);
  if (!customElements.get('crew-strip')) customElements.define('crew-strip', CrewStrip);
}
