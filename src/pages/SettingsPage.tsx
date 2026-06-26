import Toolbar from '../components/Toolbar'
import './SettingsPage.css'

interface SettingsPageProps {
  onBackHome: () => void
  onOpenTaxonomy: () => void
}

const SettingsPage = ({ onBackHome, onOpenTaxonomy }: SettingsPageProps) => {
  return (
    <main className="settings-page">
      <Toolbar
        aria-label="Settings toolbar"
        left={(
          <button type="button" onClick={onBackHome} aria-label="Go home">
            Home
          </button>
        )}
      />

      <section className="settings-page-body" aria-label="Settings">
        <div className="settings-page-heading">
          <p>Settings</p>
          <h1>App configuration</h1>
        </div>

        <button
          type="button"
          className="settings-page-card"
          onClick={onOpenTaxonomy}
          aria-label="Open Taxonomy Management"
        >
          <span>Taxonomy Management</span>
          <small>Manage canonical tags, hierarchy, descriptions, and colors.</small>
        </button>
      </section>
    </main>
  )
}

export default SettingsPage
