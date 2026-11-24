// @ts-check
import { defineConfig } from 'astro/config';

export default defineConfig({
  site: 'https://codehead.sh',
  trailingSlash: 'always',
  markdown: {
    shikiConfig: { theme: 'catppuccin-mocha', wrap: false },
  },
});
