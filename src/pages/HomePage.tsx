import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import './HomePage.css'

interface HomePageProps {
  onOpenChat: () => void
  onOpenLogs: (focusedLogEventId?: string) => void
  onOpenSettings: () => void
}

const placeholderLogs = [
  { id: 'log-morning-check-in', title: 'Morning check-in', meta: 'Today · mood, focus' },
  { id: 'log-evening-review', title: 'Evening review', meta: 'Yesterday · reflection' },
  { id: 'log-training-note', title: 'Training note', meta: 'This week · energy' },
]

const placeholderThreads = [
  { id: 'thread-journal-search', title: 'Journal search', meta: 'Recent conversation' },
  { id: 'thread-patterns', title: 'Recurring patterns', meta: 'Open in chat' },
  { id: 'thread-weekly-review', title: 'Weekly review', meta: 'Continue thread' },
]

const placeholderEntries = [
  { id: 'entry-1', title: 'Planning notes', meta: 'Recent entry' },
  { id: 'entry-2', title: 'Work session', meta: 'Recent entry' },
  { id: 'entry-3', title: 'Personal reflection', meta: 'Recent entry' },
]

const formatPromptTime = () =>
  new Date().toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  })

const HomePage = ({ onOpenChat, onOpenLogs, onOpenSettings }: HomePageProps) => {
  const [promptTime, setPromptTime] = useState(formatPromptTime)
  const [composerText, setComposerText] = useState('')
  const composerTextareaRef = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    const interval = window.setInterval(() => {
      setPromptTime(formatPromptTime())
    }, 30_000)

    return () => window.clearInterval(interval)
  }, [])

  useLayoutEffect(() => {
    const textarea = composerTextareaRef.current
    if (!textarea) return

    textarea.style.height = 'auto'
    textarea.style.height = `${textarea.scrollHeight}px`
  }, [composerText])

  return (
    <main className="home-page">
      <button
        className="home-settings-button"
        onClick={onOpenSettings}
        aria-label="Open settings"
        title="Settings"
      >
        ⚙
      </button>

      <section className="home-panels" aria-label="Home dashboard panels">
        <section className="home-panel">
          <div className="home-panel-header">
            <p>Panel A</p>
            <h2>Recent Logs</h2>
          </div>
          <div className="home-list">
            {placeholderLogs.map((log) => (
              <button key={log.id} className="home-list-item" onClick={() => onOpenLogs(log.id)}>
                <span>{log.title}</span>
                <small>{log.meta}</small>
              </button>
            ))}
          </div>
        </section>

        <section className="home-panel">
          <div className="home-panel-header">
            <p>Panel B</p>
            <h2>Recent Threads</h2>
          </div>
          <div className="home-list">
            {placeholderThreads.map((thread) => (
              <button key={thread.id} className="home-list-item" onClick={onOpenChat}>
                <span>{thread.title}</span>
                <small>{thread.meta}</small>
              </button>
            ))}
          </div>
        </section>

        <section className="home-panel">
          <div className="home-panel-header">
            <p>Panel C</p>
            <h2>Recent Entries</h2>
          </div>
          <div className="home-list">
            {placeholderEntries.map((entry) => (
              <div key={entry.id} className="home-list-item home-list-item-static">
                <span>{entry.title}</span>
                <small>{entry.meta}</small>
              </div>
            ))}
          </div>
        </section>
      </section>

      <section className="home-composer" aria-label="Composer">
        <div className="home-composer-label">Panel D</div>
        <div className="home-composer-row">
          <label className="home-terminal-input">
            <span className="home-terminal-prompt">{promptTime} &gt;</span>
            <textarea
              ref={composerTextareaRef}
              value={composerText}
              onChange={(event) => setComposerText(event.target.value)}
              placeholder="ENTER writes a new log event. CTRL+ENTER starts a new chat."
              rows={1}
            />
          </label>
          <button onClick={onOpenChat}>Open Chat</button>
        </div>
      </section>
    </main>
  )
}

export default HomePage
