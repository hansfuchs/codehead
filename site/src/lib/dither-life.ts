/**
 * Loading dither: Conway's Game of Life in 2×2 px cells behind the element's content, while the
 * element has `aria-busy="true"`. A few random sparks per generation keep the board alive, so
 * the pattern never settles into still lifes or dies out.
 *
 * Attach with `ditherLife` (Svelte `{@attach}`). The attribute drives it: set or remove
 * `aria-busy` by hand or via a prop. Tune via CSS custom properties:
 * - `--life-color`: cell color (default: the element's `color`)
 * - `--life-alpha`: cell opacity (default: 0.32)
 * Reduced motion: 1 static random board, 0 generations.
 * Sets `data-life` on the element: callers check it to attach once.
 */

import { paintCells } from "./dither-spotlight.ts";

const CELL = 2;
const STEP_MS = 90;
/** Share of live cells in a fresh board. */
const SEED = 0.3;
/** Below this share of live cells: new seed. */
const MIN_ALIVE = 0.06;
/** Sparks per generation: random 3-cell blinkers, 1 per this many cells. */
const SPARK_EVERY = 400;

/** 1 board of `cols` × `rows`, row by row, 1 = alive. */
export type Board = Uint8Array;

export function seed(
  cols: number,
  rows: number,
  random: () => number = Math.random
): Board {
  const board = new Uint8Array(cols * rows);
  for (let i = 0; i < board.length; i++) board[i] = random() < SEED ? 1 : 0;
  return board;
}

/** 1 generation on a torus (edges wrap). B3/S23. */
export function step(board: Board, cols: number, rows: number): Board {
  const next = new Uint8Array(board.length);
  for (let row = 0; row < rows; row++) {
    const up = ((row + rows - 1) % rows) * cols;
    const at = row * cols;
    const down = ((row + 1) % rows) * cols;
    for (let col = 0; col < cols; col++) {
      const left = (col + cols - 1) % cols;
      const right = (col + 1) % cols;
      const n =
        (board[up + left] ?? 0) +
        (board[up + col] ?? 0) +
        (board[up + right] ?? 0) +
        (board[at + left] ?? 0) +
        (board[at + right] ?? 0) +
        (board[down + left] ?? 0) +
        (board[down + col] ?? 0) +
        (board[down + right] ?? 0);
      next[at + col] = n === 3 || (n === 2 && board[at + col] === 1) ? 1 : 0;
    }
  }
  return next;
}

/** Random jitter: sparks (3-cell blinkers, they grow) + new seed of a near-empty board. */
export function jitter(
  board: Board,
  cols: number,
  rows: number,
  random: () => number = Math.random
): Board {
  let alive = 0;
  for (const cell of board) alive += cell;
  if (alive < board.length * MIN_ALIVE) return seed(cols, rows, random);
  const sparks = Math.max(1, Math.round(board.length / SPARK_EVERY));
  for (let s = 0; s < sparks; s++) {
    const col = Math.floor(random() * cols);
    const row = Math.floor(random() * rows);
    const across = random() < 0.5;
    for (let d = -1; d <= 1; d++) {
      const c = across ? (col + d + cols) % cols : col;
      const r = across ? row : (row + d + rows) % rows;
      board[r * cols + c] = 1;
    }
  }
  return board;
}

/** Attaches the loading dither to one element. Returns a cleanup function. */
export function ditherLife(host: HTMLElement): () => void {
  const canvas = document.createElement("canvas");
  canvas.setAttribute("aria-hidden", "true");
  canvas.style.cssText =
    "position:absolute;inset:0;width:100%;height:100%;z-index:-1;pointer-events:none;image-rendering:pixelated;display:none";
  if (getComputedStyle(host).position === "static")
    host.style.position = "relative";
  host.style.isolation = "isolate";
  host.prepend(canvas);
  host.toggleAttribute("data-life", true);

  const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");
  const forcedColors = matchMedia("(forced-colors: active)");

  let cols = 0;
  let rows = 0;
  let board: Board = new Uint8Array(0);
  let color = "";
  let timer = 0;
  let running = false;

  function paint() {
    paintCells(
      canvas,
      cols,
      rows,
      color,
      (col, row) => board[row * cols + col] === 1
    );
  }

  function measure() {
    const nextCols = Math.max(1, Math.ceil(host.clientWidth / CELL));
    const nextRows = Math.max(1, Math.ceil(host.clientHeight / CELL));
    if (nextCols === cols && nextRows === rows) return;
    cols = nextCols;
    rows = nextRows;
    board = seed(cols, rows);
  }

  function tick() {
    board = jitter(step(board, cols, rows), cols, rows);
    paint();
  }

  function start() {
    if (running) return;
    running = true;
    const style = getComputedStyle(host);
    color = style.getPropertyValue("--life-color").trim() || style.color;
    canvas.style.opacity =
      style.getPropertyValue("--life-alpha").trim() || "0.32";
    measure();
    board = seed(cols, rows);
    paint();
    canvas.style.display = "block";
    if (!reducedMotion.matches) timer = window.setInterval(tick, STEP_MS);
  }

  function stop() {
    if (!running) return;
    running = false;
    clearInterval(timer);
    timer = 0;
    canvas.style.display = "none";
  }

  function update() {
    const busy = host.getAttribute("aria-busy") === "true";
    if (busy && !forcedColors.matches) start();
    else stop();
  }

  const resize = new ResizeObserver(() => {
    if (!running) return;
    measure();
    paint();
  });
  resize.observe(host);

  const mutation = new MutationObserver(update);
  mutation.observe(host, { attributes: true, attributeFilter: ["aria-busy"] });
  forcedColors.addEventListener("change", update);
  update();

  return () => {
    stop();
    resize.disconnect();
    mutation.disconnect();
    forcedColors.removeEventListener("change", update);
    host.removeAttribute("data-life");
    canvas.remove();
  };
}
