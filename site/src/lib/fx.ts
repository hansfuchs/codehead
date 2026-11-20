/**
 * Dither effects of the site, ported from the portfolio (packages/ui/src/dither):
 * - `[data-dither]`: ordered-dither spotlight around the pointer on hover / focus
 * - `<dither-glow>`: static elliptic glow in 2×2 px Bayer cells
 * - `[data-life]`: Game of Life cells behind the element while it has `aria-busy="true"`
 * - `<ascii-monolith>`: the mascot, a monolith ray marched in braille, its eyes live (src/lib/monolith.ts)
 * - `<crew-stage>`, `<crew-strip>`: the agent avatars and a scripted thread (src/lib/crew.ts)
 */
import { initDitherSpotlights } from './dither-spotlight.ts';
import { defineDitherGlow } from './dither-glow.ts';
import { ditherLife } from './dither-life.ts';
import { defineAsciiMonolith } from './monolith.ts';
import { defineCrew } from './crew.ts';

defineDitherGlow();
defineAsciiMonolith();
defineCrew();
initDitherSpotlights();
document.querySelectorAll<HTMLElement>('[data-life]').forEach(ditherLife);
