import './NotImplementedPage.css'

interface NotImplementedPageProps {
  title: string
  description?: string
  onBackHome: () => void
}

const NotImplementedPage = ({ title, description, onBackHome }: NotImplementedPageProps) => {
  return (
    <main className="not-implemented-page">
      <section className="not-implemented-card">
        <p className="not-implemented-kicker">Coming soon</p>
        <h1>{title}</h1>
        {description && <p>{description}</p>}
        <button onClick={onBackHome}>Back home</button>
      </section>
    </main>
  )
}

export default NotImplementedPage
