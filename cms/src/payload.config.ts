import { postgresAdapter } from '@payloadcms/db-postgres'
import { sqliteAdapter } from '@payloadcms/db-sqlite'
import { lexicalEditor } from '@payloadcms/richtext-lexical'
import path from 'path'
import { buildConfig } from 'payload'
import { fileURLToPath } from 'url'
import sharp from 'sharp'

import { Faqs } from './collections/Faqs'
import { Media } from './collections/Media'
import { Users } from './collections/Users'

const filename = fileURLToPath(import.meta.url)
const dirname = path.dirname(filename)
const databaseURL = process.env.DATABASE_URL || 'file:./cms.db'
const database = databaseURL.startsWith('postgres')
  ? postgresAdapter({
      pool: {
        connectionString: databaseURL,
      },
    })
  : sqliteAdapter({
      client: {
        url: databaseURL,
      },
    })

export default buildConfig({
  admin: {
    user: Users.slug,
    // Rigel's instrument palette is dark by default; the custom sheet keeps the amber accent
    // and the warm ink surfaces in both themes.
    theme: 'dark',
    meta: {
      titleSuffix: ' · Rigel',
      icons: [{ rel: 'icon', type: 'image/svg+xml', url: '/rigel.svg' }],
    },
    components: {
      graphics: {
        Logo: '/components/RigelBrand#RigelLogo',
        Icon: '/components/RigelBrand#RigelIcon',
      },
      afterNavLinks: ['/components/RigelCloud#RigelCloudNav'],
      afterDashboard: ['/components/RigelCloud#RigelDashboard'],
    },
    importMap: {
      baseDir: path.resolve(dirname),
    },
  },
  collections: [Users, Media, Faqs],
  editor: lexicalEditor(),
  secret: process.env.PAYLOAD_SECRET || '',
  serverURL: process.env.PAYLOAD_PUBLIC_SERVER_URL || 'http://localhost:3000',
  typescript: {
    outputFile: path.resolve(dirname, 'payload-types.ts'),
  },
  db: database,
  sharp,
  plugins: [],
})
