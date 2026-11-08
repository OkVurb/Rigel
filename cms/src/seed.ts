import 'dotenv/config'
import { getPayload } from 'payload'

import config from './payload.config'
import { faqSeeds } from './seed-data'

try {
  const payload = await getPayload({ config })

  for (const faq of faqSeeds) {
    const existing = await payload.find({
      collection: 'faqs',
      where: { anchor: { equals: faq.anchor } },
      limit: 1,
      depth: 0,
    })

    if (existing.docs[0]) {
      await payload.update({
        collection: 'faqs',
        id: existing.docs[0].id,
        data: faq,
      })
    } else {
      await payload.create({ collection: 'faqs', data: faq })
    }
  }

  const users = await payload.count({ collection: 'users' })
  const email = process.env.PAYLOAD_ADMIN_EMAIL?.trim()
  const password = process.env.PAYLOAD_ADMIN_PASSWORD
  if (email && password && users.totalDocs === 0) {
    await payload.create({ collection: 'users', data: { email, password } })
    console.log('Created the first admin user from PAYLOAD_ADMIN_EMAIL and PAYLOAD_ADMIN_PASSWORD.')
  } else if (users.totalDocs === 0) {
    console.log('No admin user seeded; set PAYLOAD_ADMIN_EMAIL and PAYLOAD_ADMIN_PASSWORD, then rerun.')
  }

  console.log(`Seeded ${faqSeeds.length} FAQ entries.`)
  process.exit(0)
} catch (error) {
  console.error('CMS seed failed:', error)
  process.exit(1)
}
