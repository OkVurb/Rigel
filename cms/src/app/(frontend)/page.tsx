import './styles.css'

export default function HomePage() {
  return (
    <main className="home">
      <div className="content">
        <h1>Rigel content</h1>
        <p>
          The public site is static on Firebase. This admin edits the content the build reads;
          it is separate from the desktop app and its Firebase reader.
        </p>
        <div className="links">
          <a className="admin" href="/admin">
            Open the admin
          </a>
        </div>
      </div>
    </main>
  )
}
