import type { APIRoute } from 'astro';
import { getCollection } from 'astro:content';
import { nav, href } from '../nav';

/** llms.txt: what codehead is, then every docs page with its 1-line description. */
export const GET: APIRoute = async ({ site }) => {
  const pages = new Map((await getCollection('docs')).map((p) => [p.id === 'index' ? '' : p.id, p]));
  const out = [
    '# codehead',
    '',
    '> A terminal app for agent work: write the task with notes, pick named agents and a branch, then follow the thread (every message, tool run and question of its agents) and steer it, in one terminal. codehead runs the agents itself, on Claude Code, Codex, OpenCode, Goose, Cursor, Pi, Amp and other ACP agents.',
    '',
    `Full text of every page: ${new URL('/llms-full.txt', site)}`,
    '',
  ];
  for (const g of nav) {
    out.push(`## ${g.label}`, '');
    for (const item of g.items) {
      const p = pages.get(item.slug);
      out.push(`- [${item.label}](${new URL(href(item.slug), site)}): ${p?.data.description ?? ''}`);
    }
    out.push('');
  }
  return new Response(out.join('\n'), { headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
};
