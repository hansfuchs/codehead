/** Docs sidebar: groups and their pages (slug under /docs/), in reading order. */
export const nav: { label: string; items: { slug: string; label: string }[] }[] = [
  {
    label: 'Start',
    items: [
      { slug: '', label: 'Introduction' },
      { slug: 'requirements', label: 'Requirements' },
      { slug: 'installation', label: 'Installation' },
      { slug: 'quick-start', label: 'Quick start' },
    ],
  },
  {
    label: 'Concepts',
    items: [
      { slug: 'concepts/tasks', label: 'Tasks' },
      { slug: 'concepts/threads', label: 'Threads' },
      { slug: 'concepts/agents', label: 'Agents' },
      { slug: 'concepts/messaging', label: 'Agents together' },
      { slug: 'concepts/thread-view', label: 'Thread view' },
      { slug: 'concepts/flows', label: 'Flows' },
      { slug: 'concepts/notes', label: 'Notes and aliases' },
    ],
  },
  {
    label: 'Reference',
    items: [
      { slug: 'reference/cli', label: 'CLI' },
      { slug: 'reference/keys', label: 'Keys' },
      { slug: 'reference/configuration', label: 'Configuration' },
      { slug: 'reference/files', label: 'Files' },
    ],
  },
];

export const flat = nav.flatMap((g) => g.items);

export const href = (slug: string) => (slug ? `/docs/${slug}/` : '/docs/');
