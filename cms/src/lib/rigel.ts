// The Firebase project behind Rigel's public site, catalogue and phone sync. The admin links to
// it read-only; no Firebase credential is stored in the CMS. A deployment that uses another
// project sets RIGEL_FIREBASE_PROJECT_ID and every link follows.
const configured = process.env.RIGEL_FIREBASE_PROJECT_ID?.trim()

export const RIGEL_FIREBASE_PROJECT = configured || 'rigel-25128'

const consoleRoot = `https://console.firebase.google.com/project/${RIGEL_FIREBASE_PROJECT}`

export const RIGEL_CLOUD_LINKS = [
  { label: 'Firebase overview', href: `${consoleRoot}/overview` },
  {
    label: 'Realtime Database',
    href: `${consoleRoot}/database/${RIGEL_FIREBASE_PROJECT}-default-rtdb/data`,
  },
  { label: 'Authentication', href: `${consoleRoot}/authentication/users` },
  { label: 'Hosting', href: `${consoleRoot}/hosting/main` },
  { label: 'Firestore rules', href: `${consoleRoot}/firestore/rules` },
] as const
