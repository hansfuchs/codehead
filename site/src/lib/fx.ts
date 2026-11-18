/**
 * Dither effects of the site, ported from the portfolio (packages/ui/src/dither):
 * - `[data-dither]`: ordered-dither spotlight around the pointer on hover / focus
 * - `<dither-glow>`: static elliptic glow in 2×2 px Bayer cells
 * - `[data-life]`: Game of Life cells behind the element while it has `aria-busy="true"`
 */
import { initDitherSpotlights } from './dither-spotlight.ts';
import { defineDitherGlow } from './dither-glow.ts';
import { ditherLife } from './dither-life.ts';

defineDitherGlow();
initDitherSpotlights();
document.querySelectorAll<HTMLElement>('[data-life]').forEach(ditherLife);
