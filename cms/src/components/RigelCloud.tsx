import { RIGEL_CLOUD_LINKS, RIGEL_FIREBASE_PROJECT } from '../lib/rigel'
import { RigelMark } from './RigelBrand'

const GOOGLE_ANALYTICS_ID = 'G-XCN9DVHGV7'
const SEARCH_CONSOLE_PROPERTY = 'https://askrigel.com/'
const SEARCH_CONSOLE_VERIFICATION_URL = 'https://search.google.com/search-console/ownership?resource_id=https%3A%2F%2Faskrigel.com%2F'

function CloudLinks({ className }: { className: string }) {
  return RIGEL_CLOUD_LINKS.map((link) => (
    <a
      className={className}
      href={link.href}
      key={link.href}
      rel="noreferrer"
      target="_blank"
    >
      <span>{link.label}</span>
      <span aria-hidden="true">→</span>
    </a>
  ))
}

/** After the collection links: the project the public site and catalogue actually live in. */
export function RigelCloudNav() {
  return (
    <div className="rigel-cloud-nav">
      <p className="rigel-cloud-nav__label">Rigel Cloud</p>
      <p className="rigel-cloud-nav__project">{RIGEL_FIREBASE_PROJECT}</p>
      <CloudLinks className="rigel-cloud-nav__link" />
    </div>
  )
}

/** On the dashboard: the same links with enough room to say what they are for. */
export function RigelDashboard() {
  return (
    <div className="rigel-dashboard-stack">
    <section className="rigel-dashboard">
      <div className="rigel-dashboard__head">
        <RigelMark size={36} />
        <div>
          <h2>Rigel content</h2>
          <p>
            FAQs and media for the public site are authored here. The catalogue, accounts and
            hosting live in the Rigel Firebase project; these links open its console in a new tab.
          </p>
        </div>
      </div>
      <div className="rigel-dashboard__links">
        <CloudLinks className="rigel-dashboard__link" />
        <a className="rigel-dashboard__link" href={`https://analytics.google.com/analytics/web/#/p${GOOGLE_ANALYTICS_ID.replace(/\D/g, '')}/reports/int realtime`} target="_blank" rel="noreferrer">
          <span>Google Analytics realtime</span><span aria-hidden="true">→</span>
        </a>
        <a className="rigel-dashboard__link" href="https://search.google.com/search-console" target="_blank" rel="noreferrer">
          <span>Google Search Console</span><span aria-hidden="true">→</span>
        </a>
      </div>
    </section>
    <section className="rigel-dashboard rigel-dashboard--metrics" aria-label="Analytics and sponsorship status">
      <div className="rigel-dashboard__head">
        <div>
          <h2>Measurement and sponsorship</h2>
          <p>Live status from the integrations currently configured in this project. No estimated or fabricated counts.</p>
        </div>
      </div>
      <div className="rigel-dashboard__metrics">
        <article className="rigel-dashboard__metric">
          <span className="rigel-dashboard__eyebrow">Google Analytics 4</span>
          <strong>Configured</strong>
          <p>Marketing website only · {GOOGLE_ANALYTICS_ID}</p>
          <a href="https://analytics.google.com/analytics/web/" target="_blank" rel="noreferrer">Open live reports →</a>
        </article>
        <article className="rigel-dashboard__metric">
          <span className="rigel-dashboard__eyebrow">Search Console</span>
          <strong>Verification required</strong>
          <p>{SEARCH_CONSOLE_PROPERTY} was added as a property. Verify ownership in Google before reports and sitemap submission become available.</p>
          <a href={SEARCH_CONSOLE_VERIFICATION_URL} target="_blank" rel="noreferrer">Verify property →</a>
        </article>
        <article className="rigel-dashboard__metric">
          <span className="rigel-dashboard__eyebrow">Firebase Analytics</span>
          <strong>Not configured</strong>
          <p>The web reader has no Firebase Analytics SDK or measurement ID. Existing Firebase auth and data remain unchanged.</p>
          <a href={`https://console.firebase.google.com/project/${RIGEL_FIREBASE_PROJECT}/analytics` } target="_blank" rel="noreferrer">Open Firebase Analytics →</a>
        </article>
        <article className="rigel-dashboard__metric">
          <span className="rigel-dashboard__eyebrow">Gravity publisher</span>
          <strong>Test mode · earnings unavailable</strong>
          <p>The broker serves test ads until publisher approval. This project has no earnings/reporting feed; impression tracking is handled by Gravity.</p>
          <a href="https://trygravity.com" target="_blank" rel="noreferrer">Open Gravity publisher →</a>
        </article>
      </div>
    </section>
    </div>
  )
}
