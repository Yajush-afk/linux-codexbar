import "./App.css";

function App() {
  return (
    <main className="app-shell">
      <section className="hero-card">
        <p className="eyebrow">Tray-First Fedora Build</p>
        <h1>Linux CodexBar</h1>
        <p className="lead">
          Tray-only Fedora KDE app for OpenCode, Codex, and Claude usage windows.
        </p>
      </section>

      <section className="content-grid">
        <article className="panel">
          <h2>Current status</h2>
          <ul>
            <li>Tray menu runtime implemented in Rust.</li>
            <li>OpenCode uses manual cookie-header auth from config.</li>
            <li>Codex and Claude use local OAuth credential files.</li>
          </ul>
        </article>

        <article className="panel">
          <h2>How to use it</h2>
          <ul>
            <li>Run the desktop app and look in the KDE system tray.</li>
            <li>Open `config.json` from the tray menu.</li>
            <li>Reload config, then refresh providers from the tray.</li>
          </ul>
        </article>

        <article className="panel">
          <h2>Important note</h2>
          <p>
            On Linux, tray click events are unreliable, so the app is intentionally menu-driven.
          </p>
        </article>
      </section>
    </main>
  );
}

export default App;
