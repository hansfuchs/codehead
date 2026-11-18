/**
 * Ordered-dither spotlight on hover/focus, ported from the e2b.dev buttons.
 *
 * A canvas behind the element's content draws 2×2 px cells. A cell is lit
 * when a smoothstep radial falloff around the pointer beats its 8×8 Bayer
 * threshold, so the spotlight edge breaks up into a dither pattern.
 *
 * Opt in with `data-dither` on any element, or call `ditherSpotlight` (sets `data-dither`, so
 * the `frame` utility drops its hover tint). Tune via CSS custom properties:
 * - `--dither-color`: cell color (default: the element's `color`)
 * - `--dither-alpha`: cell opacity at full hover (default: 0.16)
 * - `--dither-radius`: spotlight radius in px (default: 82)
 * Sets `--dither-progress` (0..1) on the element for CSS transitions.
 */

export const BAYER_8 = [
  [0, 32, 8, 40, 2, 34, 10, 42],
  [48, 16, 56, 24, 50, 18, 58, 26],
  [12, 44, 4, 36, 14, 46, 6, 38],
  [60, 28, 52, 20, 62, 30, 54, 22],
  [3, 35, 11, 43, 1, 33, 9, 41],
  [51, 19, 59, 27, 49, 17, 57, 25],
  [15, 47, 7, 39, 13, 45, 5, 37],
  [63, 31, 55, 23, 61, 29, 53, 21],
];

/**
 * Draws a cell grid at 1 px per cell: lit cells in `color`, the rest clear. Sizes the canvas to
 * `cols` × `rows`; CSS scales it up, with `image-rendering: pixelated` for sharp cell edges.
 * 1 `putImageData` + 1 fill, not 1 `fillRect` per lit cell.
 */
export function paintCells(
  canvas: HTMLCanvasElement,
  cols: number,
  rows: number,
  color: string,
  lit: (col: number, row: number) => boolean
): void {
  const ctx = canvas.getContext("2d");
  if (!ctx || !cols || !rows) return;
  if (canvas.width !== cols) canvas.width = cols;
  if (canvas.height !== rows) canvas.height = rows;
  const image = ctx.createImageData(cols, rows);
  // Alpha byte of each pixel: opaque = lit.
  for (let row = 0, i = 3; row < rows; row++) {
    for (let col = 0; col < cols; col++, i += 4)
      if (lit(col, row)) image.data[i] = 255;
  }
  ctx.putImageData(image, 0, 0);
  // `source-in`: the fill keeps the alpha of the lit cells and sets their color.
  ctx.globalCompositeOperation = "source-in";
  ctx.fillStyle = color;
  ctx.fillRect(0, 0, cols, rows);
  ctx.globalCompositeOperation = "source-over";
}

const CELL = 2;
const FADE_MS = 250;
const FOLLOW_MS = 40;

/** Attaches the spotlight to one element. Returns a cleanup function. */
export function ditherSpotlight(host: HTMLElement): () => void {
  const canvas = document.createElement("canvas");
  canvas.setAttribute("aria-hidden", "true");
  canvas.style.cssText =
    "position:absolute;inset:0;width:100%;height:100%;z-index:-1;pointer-events:none";
  if (getComputedStyle(host).position === "static")
    host.style.position = "relative";
  host.style.isolation = "isolate";
  const marked = !host.hasAttribute("data-dither");
  host.toggleAttribute("data-dither", true);
  host.prepend(canvas);
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    if (marked) host.removeAttribute("data-dither");
    canvas.remove();
    return () => {};
  }

  const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");
  const forcedColors = matchMedia("(forced-colors: active)");
  const abort = new AbortController();
  const { signal } = abort;

  let width = 0;
  let height = 0;
  let dpr = 1;
  // Drawn pointer position eases toward the target position.
  let x = 0;
  let y = 0;
  let targetX = 0;
  let targetY = 0;
  // Idle wobble offset while hovered.
  let wobbleX = 0;
  let wobbleY = 0;
  let progress = 0;
  let fadeFrom = 0;
  let fadeTo = 0;
  let fadeStart = 0;
  let activeSince = 0;
  let hovered = false;
  let focused = false;
  let visible = true;
  let frame = 0;
  let lastTime = 0;
  let touchTimer = 0;
  // Read on state changes, not per frame: 0 forced style recalcs in the loop.
  let color = "";
  let alpha = 0.16;
  let radius = 82;

  function readStyle() {
    const style = getComputedStyle(host);
    alpha = parseFloat(style.getPropertyValue("--dither-alpha")) || 0.16;
    radius = parseFloat(style.getPropertyValue("--dither-radius")) || 82;
    color = style.getPropertyValue("--dither-color").trim() || style.color;
  }

  function draw() {
    ctx!.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx!.clearRect(0, 0, width, height);
    host.style.setProperty("--dither-progress", String(progress));
    if (progress < 0.001 || !width || !height) return;

    ctx!.fillStyle = color;
    ctx!.globalAlpha = progress * alpha;

    const r = Math.min(radius, width * 0.75) * (0.72 + 0.28 * progress);
    const cx = x + wobbleX;
    const cy = y + wobbleY;
    // Only the cells inside the bounding box of the spotlight can light up.
    const col0 = Math.max(0, Math.floor((cx - r) / CELL));
    const col1 = Math.min(Math.ceil(width / CELL), Math.ceil((cx + r) / CELL));
    const row0 = Math.max(0, Math.floor((cy - r) / CELL));
    const row1 = Math.min(Math.ceil(height / CELL), Math.ceil((cy + r) / CELL));
    for (let row = row0; row < row1; row++) {
      const dy = row * CELL + 1 - cy;
      for (let col = col0; col < col1; col++) {
        const d = Math.hypot(col * CELL + 1 - cx, dy);
        const t = Math.max(0, 1 - d / r);
        if (
          t * t * (3 - 2 * t) >
          ((BAYER_8[row % 8]?.[col % 8] ?? 0) + 0.5) / 64
        ) {
          ctx!.fillRect(col * CELL, row * CELL, CELL, CELL);
        }
      }
    }
    ctx!.globalAlpha = 1;
  }

  function tick(now: number) {
    frame = 0;
    if (!visible || document.hidden) return;
    const dt = Math.min(40, lastTime ? now - lastTime : 16);
    lastTime = now;

    if (reducedMotion.matches) {
      progress = fadeTo;
      x = width / 2;
      y = height / 2;
      wobbleX = wobbleY = 0;
    } else {
      if (progress !== fadeTo) {
        const t = Math.min(1, Math.max(0, (now - fadeStart) / FADE_MS));
        progress = fadeFrom + (fadeTo - fadeFrom) * (1 - (1 - t) ** 3);
        if (t === 1) progress = fadeTo;
      }
      const k = 1 - Math.exp(-dt / FOLLOW_MS);
      x += (targetX - x) * k;
      y += (targetY - y) * k;
      if (progress > 0.001 && (hovered || focused)) {
        const s = (now - activeSince) / 1000;
        wobbleX = 14 * Math.sin((s * Math.PI * 2) / 4.8) * progress;
        wobbleY = 4 * Math.sin((s * Math.PI * 2) / 3.6) * progress;
      } else if (progress === 0) {
        wobbleX = wobbleY = 0;
      }
    }

    draw();

    const animating =
      progress !== fadeTo ||
      (progress > 0 && (hovered || focused)) ||
      Math.abs(targetX - x) + Math.abs(targetY - y) > 0.1;
    if (!reducedMotion.matches && !forcedColors.matches && animating) {
      frame = requestAnimationFrame(tick);
    } else {
      lastTime = 0;
    }
  }

  function schedule() {
    if (!frame && visible && !document.hidden)
      frame = requestAnimationFrame(tick);
  }

  function update() {
    const disabled = host.matches(":disabled, [aria-disabled='true']");
    const next =
      !disabled && !forcedColors.matches && (hovered || focused) ? 1 : 0;
    if (next !== fadeTo) {
      if (next) readStyle();
      fadeFrom = progress;
      fadeTo = next;
      fadeStart = performance.now();
    }
    schedule();
  }

  function track(e: PointerEvent) {
    if (reducedMotion.matches) return;
    const rect = host.getBoundingClientRect();
    targetX = ((e.clientX - rect.left) * width) / rect.width;
    targetY = ((e.clientY - rect.top) * height) / rect.height;
    // Jump instead of easing in from the previous spot.
    if (progress < 0.01) {
      x = targetX;
      y = targetY;
    }
    schedule();
  }

  function enter(e: PointerEvent) {
    hovered = true;
    activeSince = performance.now();
    track(e);
    update();
  }

  function leave() {
    hovered = false;
    update();
  }

  function touchEnd(e: PointerEvent) {
    if (e.pointerType === "touch") touchTimer = window.setTimeout(leave, 300);
  }

  host.addEventListener(
    "pointerenter",
    (e) => e.pointerType !== "touch" && enter(e),
    {
      signal,
    }
  );
  host.addEventListener("pointermove", (e) => hovered && track(e), { signal });
  host.addEventListener("pointerleave", leave, { signal });
  host.addEventListener(
    "pointerdown",
    (e) => {
      if (e.pointerType !== "touch") return;
      clearTimeout(touchTimer);
      enter(e);
    },
    { signal }
  );
  host.addEventListener("pointerup", touchEnd, { signal });
  host.addEventListener("pointercancel", touchEnd, { signal });
  host.addEventListener(
    "focus",
    () => {
      focused = host.matches(":focus-visible");
      if (focused) {
        activeSince = performance.now();
        targetX = width / 2;
        targetY = height / 2;
      }
      update();
    },
    { signal }
  );
  host.addEventListener("blur", () => ((focused = false), update()), {
    signal,
  });
  reducedMotion.addEventListener("change", schedule, { signal });
  forcedColors.addEventListener("change", update, { signal });

  const resize = new ResizeObserver(() => {
    width = host.clientWidth;
    height = host.clientHeight;
    dpr = Math.min(devicePixelRatio || 1, 2);
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    x = targetX = width / 2;
    y = targetY = height / 2;
    schedule();
  });
  resize.observe(host);

  function onVisibility() {
    if (!visible || document.hidden) {
      cancelAnimationFrame(frame);
      frame = 0;
      lastTime = 0;
    } else {
      schedule();
    }
  }
  const intersection = new IntersectionObserver(([entry]) => {
    visible = entry?.isIntersecting ?? false;
    onVisibility();
  });
  intersection.observe(host);
  document.addEventListener("visibilitychange", onVisibility, { signal });

  const mutation = new MutationObserver(update);
  mutation.observe(host, {
    attributes: true,
    attributeFilter: ["disabled", "aria-disabled"],
  });

  return () => {
    abort.abort();
    cancelAnimationFrame(frame);
    clearTimeout(touchTimer);
    resize.disconnect();
    intersection.disconnect();
    mutation.disconnect();
    host.style.removeProperty("--dither-progress");
    if (marked) host.removeAttribute("data-dither");
    canvas.remove();
  };
}

/** Attaches the spotlight to every `[data-dither]` element under `root`. */
export function initDitherSpotlights(root: ParentNode = document): () => void {
  const cleanups = [...root.querySelectorAll<HTMLElement>("[data-dither]")].map(
    ditherSpotlight
  );
  return () => {
    for (const cleanup of cleanups) cleanup();
  };
}
