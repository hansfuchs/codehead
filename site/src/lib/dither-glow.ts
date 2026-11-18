import { BAYER_8, paintCells } from "./dither-spotlight.ts";

const CELL = 2;

/**
 * Static elliptic glow, broken up into an 8×8 Bayer dither. Fills its box: the ellipse
 * touches the edges. `data-peak` (0..1) sets the fill value at the center (default: 0.5).
 * Cell color: the element's `color`. Canvas at 1 px per cell, scaled up pixelated.
 */
export class DitherGlow extends HTMLElement {
  #canvas?: HTMLCanvasElement;
  #observer?: ResizeObserver;

  connectedCallback() {
    this.#canvas = this.querySelector<HTMLCanvasElement>("canvas")!;
    this.#observer = new ResizeObserver(() => this.#draw());
    this.#observer.observe(this);
  }

  disconnectedCallback() {
    this.#observer?.disconnect();
  }

  // A cell is lit when the smoothstep falloff of its elliptic distance beats its Bayer threshold.
  #draw() {
    if (!this.#canvas) return;
    const width = this.clientWidth;
    const height = this.clientHeight;
    const peak = Number(this.dataset["peak"]) || 0.5;
    const rx = width / 2;
    const ry = height / 2;
    paintCells(
      this.#canvas,
      Math.ceil(width / CELL),
      Math.ceil(height / CELL),
      getComputedStyle(this).color,
      (col, row) => {
        const dx = (col * CELL + CELL / 2 - rx) / rx;
        const dy = (row * CELL + CELL / 2 - ry) / ry;
        const t = Math.max(0, 1 - Math.sqrt(dx * dx + dy * dy));
        return (
          peak * t * t * (3 - 2 * t) >
          ((BAYER_8[row % 8]?.[col % 8] ?? 0) + 0.5) / 64
        );
      }
    );
  }
}

/** Registers <dither-glow> once. */
export function defineDitherGlow(): void {
  if (!customElements.get("dither-glow")) {
    customElements.define("dither-glow", DitherGlow);
  }
}
