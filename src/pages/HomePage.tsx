import { useCallback, useState } from 'react'
import HomeComposer from '../components/HomeComposer'
import RecentListPanel from '../components/RecentListPanel'
import { useMeasuredRecentList } from '../hooks/useMeasuredRecentList'
import { useTagTaxonomy } from '../hooks/useTagTaxonomy'
import { apiService, type Entry, type LogEvent } from '../services/api'
import type { Thread } from '../types'
import './HomePage.css'

interface HomePageProps {
  onOpenChat: (threadId?: string) => void
  onOpenLogs: (focusedLogEventId?: string) => void
  onOpenSettings: () => void
}

const shortDayCodes = ['Sun', 'M', 'T', 'W', 'R', 'F', 'Sat']

const pad2 = (value: number) => value.toString().padStart(2, '0')

const formatDate = (value: string) => {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return date.toLocaleDateString()
}

const formatLogDateTime = (value: string) => {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return `${shortDayCodes[date.getDay()]} ${pad2(date.getMonth() + 1)}/${pad2(date.getDate())} ${pad2(date.getHours())}:${pad2(date.getMinutes())}`
}

const makePreview = (text: string, maxLength = 140) => {
  const normalizedText = text.trim()
  if (normalizedText.length <= maxLength) return normalizedText
  return `${normalizedText.slice(0, maxLength).trim()}…`
}

const getEntryKey = (entry: Entry) => entry.entry_id || `${entry.date}:${entry.title}`

const HomePage = ({ onOpenChat, onOpenLogs, onOpenSettings }: HomePageProps) => {
  const [expandedEntryIds, setExpandedEntryIds] = useState<Set<string>>(() => new Set())
  const tagTaxonomy = useTagTaxonomy()

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
              <small>{formatLogDateTime(log.datetime)}{log.tags.length > 0 ? ` · ${log.tags.join(', ')}` : ''}</small>
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

      <HomeComposer
        tagSuggestions={tagTaxonomy.tags}
        onTagsCreated={tagTaxonomy.addOptimisticTags}
        onLogCreated={() => {
          logsList.refresh()
          tagTaxonomy.refresh()
        }}
        onOpenChat={() => onOpenChat()}
      />
    </main>
  )
}

export default HomePage
