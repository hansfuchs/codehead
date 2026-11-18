import type { APIRoute } from 'astro';
import { getCollection } from 'astro:content';
import { flat } from '../nav';

/** Every docs page as Markdown, in sidebar order. */
export const GET: APIRoute = async () => {
  const pages = new Map((await getCollection('docs')).map((p) => [p.id === 'index' ? '' : p.id, p]));
  const out = ['# codehead docs', ''];
  for (const item of flat) {
    const p = pages.get(item.slug);
    if (!p) continue;
    out.push(`# ${p.data.title}`, '', `> ${p.data.description}`, '', (p.body ?? '').trim(), '');
  }
  return new Response(out.join('\n'), { headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
};
