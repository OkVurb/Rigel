import React from 'react'
import './styles.css'

export const metadata = {
  description: 'Content and admin backend for the Rigel website.',
  title: 'Rigel content',
}

export default async function RootLayout(props: { children: React.ReactNode }) {
  const { children } = props

  return (
    <html lang="en">
      <body>
        <main>{children}</main>
      </body>
    </html>
  )
}
