import type { CollectionConfig } from 'payload'

export const Faqs: CollectionConfig = {
  slug: 'faqs',
  admin: {
    useAsTitle: 'question',
    defaultColumns: ['order', 'anchor', 'question', 'published'],
  },
  access: {
    read: ({ req }) => (req.user ? true : { published: { equals: true } }),
    create: ({ req }) => Boolean(req.user),
    update: ({ req }) => Boolean(req.user),
    delete: ({ req }) => Boolean(req.user),
  },
  defaultSort: 'order',
  fields: [
    {
      name: 'order',
      type: 'number',
      required: true,
      defaultValue: 0,
      index: true,
    },
    {
      name: 'anchor',
      type: 'text',
      required: true,
      unique: true,
      index: true,
      admin: {
        description: 'Stable URL fragment, for example offline. Lowercase letters, numbers and hyphens.',
      },
    },
    {
      name: 'navLabel',
      type: 'text',
      required: true,
      admin: {
        description: 'The short label in the On this page navigation.',
      },
    },
    {
      name: 'question',
      type: 'text',
      required: true,
    },
    {
      name: 'answer',
      type: 'textarea',
      required: true,
      admin: {
        description:
          'Markdown paragraphs. Links, **bold**, `code` and {{skill_count}} are supported by the static build.',
      },
    },
    {
      name: 'published',
      type: 'checkbox',
      defaultValue: true,
      index: true,
    },
  ],
}
