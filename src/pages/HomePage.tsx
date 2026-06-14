import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import RecentListPanel from '../components/RecentListPanel'
import { useMeasuredRecentList } from '../hooks/useMeasuredRecentList'
import { apiService, type Entry, type LogEvent } from '../services/api'
import type { Thread } from '../types'
import './HomePage.css'

interface HomePageProps {
  onOpenChat: (threadId?: string) => void
  onOpenLogs: (focusedLogEventId?: string) => void
  onOpenSettings: () => void
}

const formatPromptTime = () =>
  new Date().toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  })

const formatDate = (value: string) => {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return date.toLocaleDateString()
}

const makePreview = (text: string, maxLength = 140) => {
  const normalizedText = text.trim()
  if (normalizedText.length <= maxLength) return normalizedText
  return `${normalizedText.slice(0, maxLength).trim()}…`
}

const getEntryKey = (entry: Entry) => entry.entry_id || `${entry.date}:${entry.title}`

const HomePage = ({ onOpenChat, onOpenLogs, onOpenSettings }: HomePageProps) => {
  const [promptTime, setPromptTime] = useState(formatPromptTime)
  const [composerText, setComposerText] = useState('')
  const [expandedEntryIds, setExpandedEntryIds] = useState<Set<string>>(() => new Set())
  const composerTextareaRef = useRef<HTMLTextAreaElement>(null)

  const logsList = useMeasuredRecentList<LogEvent>({
    fetchItems: useCallback(
      (limit) => apiService.listLogEvents({ order: 'descending', limit }),
      [],
    ),
    estimateItemHeight: 72,
  })

  const threadsList = useMeasuredRecentList<Thread>({
    fetchItems: useCallback(async (limit) => {
      const threads = await apiService.getThreads()
      return [...threads]
        .sort((a, b) => Date.parse(b.updated_at) - Date.parse(a.updated_at))
        .slice(0, limit)
    }, []),
    estimateItemHeight: 72,
  })

  const entriesList = useMeasuredRecentList<Entry>({
    fetchItems: useCallback((limit) => apiService.getRecentEntries(limit), []),
    estimateItemHeight: 88,
  })

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

  const toggleEntryExpanded = (entryId: string) => {
    setExpandedEntryIds((current) => {
      const next = new Set(current)
      if (next.has(entryId)) {
        next.delete(entryId)
      } else {
        next.add(entryId)
      }
      return next
    })
  }

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
        <RecentListPanel
          label="Panel A"
          title="Recent Logs"
          items={logsList.items}
          loading={logsList.loading}
          error={logsList.error}
          bodyRef={logsList.bodyRef}
          onRefresh={logsList.refresh}
          getItemKey={(log) => log.log_event_id}
          renderItem={(log) => (
            <button className="home-list-item" onClick={() => onOpenLogs(log.log_event_id)}>
              <span>{log.text}</span>
              <small>{formatDate(log.datetime)}{log.tags.length > 0 ? ` · ${log.tags.join(', ')}` : ''}</small>
            </button>
          )}
        />

        <RecentListPanel
          label="Panel B"
          title="Recent Threads"
          items={threadsList.items}
          loading={threadsList.loading}
          error={threadsList.error}
          bodyRef={threadsList.bodyRef}
          onRefresh={threadsList.refresh}
          getItemKey={(thread) => thread.thread_id}
          renderItem={(thread) => (
            <button className="home-list-item" onClick={() => onOpenChat(thread.thread_id)}>
              <span>{thread.title}</span>
              <small>
                Updated {formatDate(thread.updated_at)}
                {thread.tags && thread.tags.length > 0 ? ` · ${thread.tags.join(', ')}` : ''}
              </small>
            </button>
          )}
        />

        <RecentListPanel
          label="Panel C"
          title="Recent Entries"
          items={entriesList.items}
          loading={entriesList.loading}
          error={entriesList.error}
          bodyRef={entriesList.bodyRef}
          onRefresh={entriesList.refresh}
          getItemKey={getEntryKey}
          renderItem={(entry) => {
            const entryKey = getEntryKey(entry)
            const isExpanded = expandedEntryIds.has(entryKey)

            return (
              <button
                className={`home-list-item recent-entry-item ${isExpanded ? 'expanded' : ''}`}
                onClick={() => toggleEntryExpanded(entryKey)}
                aria-expanded={isExpanded}
              >
                <div className="recent-entry-title-row">
                  <span>{entry.title || 'Untitled entry'}</span>
                  <span className="recent-entry-caret">{isExpanded ? '▾' : '▸'}</span>
                </div>
                <small>{formatDate(entry.date)}{entry.tags.length > 0 ? ` · ${entry.tags.join(', ')}` : ''}</small>
                {entry.text && (
                  isExpanded ? (
                    <div className="recent-entry-full-text">{entry.text}</div>
                  ) : (
                    <p className="recent-entry-preview">{makePreview(entry.text)}</p>
                  )
                )}
              </button>
            )
          }}
        />
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
          <button onClick={() => onOpenChat()}>Open Chat</button>
        </div>
      </section>
    </main>
  )
}

export default HomePage
