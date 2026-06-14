import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import HomeComposer from '../components/HomeComposer'
import { useTagTaxonomy } from '../hooks/useTagTaxonomy'
import { apiService, type LogEvent } from '../services/api'
import './LogPage.css'

interface LogPageProps {
  focusedLogEventId?: string
  onBackHome: () => void
  onOpenSettings: () => void
}

type LogViewType = 'all' | 'today'

const pad2 = (value: number) => value.toString().padStart(2, '0')

const parseLogDate = (value: string) => {
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? null : date
}

const formatLogPromptTime = (value: string) => {
  const date = parseLogDate(value)
  if (!date) return value
  return `${pad2(date.getHours())}:${pad2(date.getMinutes())}`
}

const getLogDayKey = (log: LogEvent) => {
  const date = parseLogDate(log.datetime)
  if (!date) return log.datetime || 'unknown-date'
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`
}

const formatLogDay = (dayKey: string) => {
  const [year, month, day] = dayKey.split('-').map(Number)
  if (!year || !month || !day) return dayKey

  const date = new Date(year, month - 1, day)
  return date.toLocaleDateString(undefined, {
    weekday: 'short',
    month: 'short',
    day: 'numeric',
    year: 'numeric',
  })
}

const groupLogsByDay = (logs: LogEvent[]) => {
  const groups: Array<{ dayKey: string; logs: LogEvent[] }> = []

  for (const log of logs) {
    const dayKey = getLogDayKey(log)
    const currentGroup = groups[groups.length - 1]

    if (currentGroup?.dayKey === dayKey) {
      currentGroup.logs.push(log)
    } else {
      groups.push({ dayKey, logs: [log] })
    }
  }

  return groups
}

const isTodayLog = (log: LogEvent) => {
  const date = parseLogDate(log.datetime)
  if (!date) return false

  const today = new Date()
  return (
    date.getFullYear() === today.getFullYear() &&
    date.getMonth() === today.getMonth() &&
    date.getDate() === today.getDate()
  )
}

const LogPage = ({
  focusedLogEventId,
  onBackHome,
  onOpenSettings,
}: LogPageProps) => {
  const tagTaxonomy = useTagTaxonomy()
  const [viewType, setViewType] = useState<LogViewType>('all')
  const [selectedTags, setSelectedTags] = useState<string[]>([])
  const [isTagFilterOpen, setIsTagFilterOpen] = useState(false)
  const [tagFilterQuery, setTagFilterQuery] = useState('')
  const [activeActionLogEventId, setActiveActionLogEventId] = useState<string | null>(null)
  const [activeFocusedLogEventId, setActiveFocusedLogEventId] = useState(focusedLogEventId)
  const [logs, setLogs] = useState<LogEvent[]>([])
  const logBodyRef = useRef<HTMLElement>(null)
  const tagFilterRef = useRef<HTMLDivElement>(null)
  const shouldScrollToFocusedLogRef = useRef(Boolean(focusedLogEventId))
  const shouldSkipNextBottomScrollRef = useRef(false)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refreshLogs = useCallback(async () => {
    try {
      setLoading(true)
      setError(null)
      const nextLogs = await apiService.listLogEvents({
        order: 'ascending',
        tags: selectedTags,
      })
      setLogs(nextLogs)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load log events')
    } finally {
      setLoading(false)
    }
  }, [selectedTags])

  useEffect(() => {
    void refreshLogs()
  }, [refreshLogs])

  useEffect(() => {
    shouldScrollToFocusedLogRef.current = Boolean(focusedLogEventId)
    setActiveFocusedLogEventId(focusedLogEventId)
  }, [focusedLogEventId])

  useEffect(() => {
    if (!isTagFilterOpen) return

    const handlePointerDown = (event: PointerEvent) => {
      const tagFilter = tagFilterRef.current
      if (!tagFilter || tagFilter.contains(event.target as Node)) return
      setIsTagFilterOpen(false)
    }

    document.addEventListener('pointerdown', handlePointerDown)

    return () => document.removeEventListener('pointerdown', handlePointerDown)
  }, [isTagFilterOpen])

  const addSelectedTag = (tag: string) => {
    setSelectedTags((currentTags) => currentTags.includes(tag) ? currentTags : [...currentTags, tag])
    setTagFilterQuery('')
  }

  const removeSelectedTag = (tag: string) => {
    setSelectedTags((currentTags) => currentTags.filter((currentTag) => currentTag !== tag))
  }

  const clearSelectedTags = () => setSelectedTags([])

  const openLogActionMenu = (log: LogEvent) => {
    shouldScrollToFocusedLogRef.current = false
    setActiveFocusedLogEventId(log.log_event_id)
    setActiveActionLogEventId((currentId) => currentId === log.log_event_id ? null : log.log_event_id)
  }

  const deleteLogEvent = async (log: LogEvent) => {
    try {
      setActiveActionLogEventId(null)
      setError(null)
      setLogs((currentLogs) => currentLogs.filter((currentLog) => currentLog.log_event_id !== log.log_event_id))
      await apiService.deleteLogEvent(log.log_event_id)
      if (activeFocusedLogEventId === log.log_event_id) {
        setActiveFocusedLogEventId(undefined)
      }
      tagTaxonomy.refresh()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete log event')
      void refreshLogs()
    }
  }

  const normalizedTagFilterQuery = tagFilterQuery.trim().toLowerCase()
  const matchingTags = tagTaxonomy.tags
    .filter((summary) => !selectedTags.includes(summary.tag))
    .filter((summary) =>
      normalizedTagFilterQuery.length === 0 || summary.tag.toLowerCase().includes(normalizedTagFilterQuery),
    )
    .slice(0, 20)
  const tagFilterLabel = selectedTags.length === 0 ? 'Tags' : `Tags: ${selectedTags.join(', ')}`
  const visibleLogs = viewType === 'today' ? logs.filter(isTodayLog) : logs
  const groupedVisibleLogs = groupLogsByDay(visibleLogs)

  useLayoutEffect(() => {
    if (loading || error) return

    const logBody = logBodyRef.current
    if (!logBody) return

    if (activeFocusedLogEventId) {
      if (shouldScrollToFocusedLogRef.current) {
        document
          .getElementById(`log-event-${activeFocusedLogEventId}`)
          ?.scrollIntoView({ block: 'center' })
        shouldScrollToFocusedLogRef.current = false
      }
      return
    }

    if (shouldSkipNextBottomScrollRef.current) {
      shouldSkipNextBottomScrollRef.current = false
      return
    }

    logBody.scrollTop = logBody.scrollHeight
  }, [activeFocusedLogEventId, error, loading, visibleLogs.length])

  return (
    <main
      className="log-page"
      onPointerDownCapture={(event) => {
        const target = event.target as HTMLElement
        const isInteractingWithLogMenu = Boolean(
          target.closest('.log-page-event-prefix') || target.closest('.log-page-log-action-menu'),
        )

        if (activeFocusedLogEventId) {
          shouldSkipNextBottomScrollRef.current = true
          setActiveFocusedLogEventId(undefined)
        }
        if (activeActionLogEventId && !isInteractingWithLogMenu) {
          setActiveActionLogEventId(null)
        }
      }}
    >
      <header className="log-page-toolbar" aria-label="Log page toolbar">
        <div className="log-page-toolbar-left">
          <button type="button" onClick={onBackHome} aria-label="Go home">
            Home
          </button>

          <label className="log-page-view-select-label">
            <span className="log-page-toolbar-label">View</span>
            <select
              value={viewType}
              onChange={(event) => setViewType(event.target.value as LogViewType)}
              aria-label="Select log view"
            >
              <option value="all">ALL</option>
              <option value="today">TODAY</option>
            </select>
          </label>

          <div className="log-page-tag-filter" ref={tagFilterRef}>
            <button
              type="button"
              className="log-page-tag-filter-button"
              onClick={() => setIsTagFilterOpen((isOpen) => !isOpen)}
              aria-expanded={isTagFilterOpen}
              aria-haspopup="listbox"
              title={tagFilterLabel}
            >
              {tagFilterLabel}
            </button>

            {isTagFilterOpen && (
              <div className="log-page-tag-filter-menu">
                <div className="log-page-tag-filter-menu-header">
                  <span>Filter tags</span>
                  <button type="button" onClick={clearSelectedTags} disabled={selectedTags.length === 0}>
                    Clear
                  </button>
                </div>

                <input
                  className="log-page-tag-filter-search"
                  value={tagFilterQuery}
                  onChange={(event) => setTagFilterQuery(event.target.value)}
                  placeholder="Type to find a tag..."
                  aria-label="Find tag to filter by"
                />

                {selectedTags.length > 0 && (
                  <div className="log-page-selected-tags" aria-label="Selected tag filters">
                    {selectedTags.map((tag) => (
                      <button
                        className="log-page-selected-tag"
                        type="button"
                        key={tag}
                        onClick={() => removeSelectedTag(tag)}
                        aria-label={`Remove ${tag} filter`}
                      >
                        <span>{tag}</span>
                        <span aria-hidden="true">×</span>
                      </button>
                    ))}
                  </div>
                )}

                {tagTaxonomy.tags.length === 0 ? (
                  <div className="log-page-tag-filter-empty">No tags yet.</div>
                ) : matchingTags.length === 0 ? (
                  <div className="log-page-tag-filter-empty">No matching tags.</div>
                ) : (
                  <div className="log-page-tag-filter-results">
                    {matchingTags.map((summary) => (
                      <button
                        className="log-page-tag-filter-option"
                        type="button"
                        key={summary.tag}
                        onClick={() => addSelectedTag(summary.tag)}
                      >
                        <span>{summary.tag}</span>
                        <small>{summary.count}</small>
                      </button>
                    ))}
                  </div>
                )}
              </div>
            )}
          </div>
        </div>

        <div className="log-page-toolbar-right">
          <input
            className="log-page-search"
            placeholder="Search logs..."
            aria-label="Search logs"
            readOnly
          />

          <button type="button" onClick={onOpenSettings} aria-label="Open settings">
            Settings
          </button>
        </div>
      </header>

      <section className="log-page-body" aria-label="Logs" ref={logBodyRef}>
        {loading && logs.length === 0 ? (
          <p className="log-page-placeholder">Loading logs…</p>
        ) : error ? (
          <p className="log-page-placeholder log-page-error">{error}</p>
        ) : visibleLogs.length === 0 ? (
          <p className="log-page-placeholder">
            {selectedTags.length > 0 ? 'No logs match those tags.' : 'No logs yet.'}
          </p>
        ) : (
          <div className="log-page-list">
            {groupedVisibleLogs.map((group) => (
              <section className="log-page-day-group" key={group.dayKey} aria-label={formatLogDay(group.dayKey)}>
                <div className="log-page-day-divider">
                  <span>{formatLogDay(group.dayKey)}</span>
                </div>

                {group.logs.map((log) => (
                  <article
                    id={`log-event-${log.log_event_id}`}
                    key={log.log_event_id}
                    className={`log-page-event ${log.log_event_id === activeFocusedLogEventId ? 'focused' : ''}`}
                    onClick={() => {
                      shouldScrollToFocusedLogRef.current = false
                      setActiveFocusedLogEventId(log.log_event_id)
                    }}
                  >
                    <p>
                      <button
                        type="button"
                        className="log-page-event-prefix"
                        onClick={(event) => {
                          event.stopPropagation()
                          openLogActionMenu(log)
                        }}
                        aria-label="Open log actions"
                        title="Open log actions"
                      >
                        <time dateTime={log.datetime}>{formatLogPromptTime(log.datetime)} &gt;</time>
                      </button>
                      <span className="log-page-event-text">{log.text}</span>
                      {log.tags.length > 0 && <small className="log-page-event-tags">{log.tags.join(', ')}</small>}
                    </p>
                    {activeActionLogEventId === log.log_event_id && (
                      <div className="log-page-log-action-menu" onClick={(event) => event.stopPropagation()}>
                        <button
                          type="button"
                          onClick={(event) => {
                            event.preventDefault()
                            event.stopPropagation()
                            void deleteLogEvent(log)
                          }}
                        >
                          Delete log
                        </button>
                      </div>
                    )}
                  </article>
                ))}
              </section>
            ))}
          </div>
        )}
      </section>

      <footer className="log-page-composer-shell" aria-label="Log composer">
        <HomeComposer
          tagSuggestions={tagTaxonomy.tags}
          onTagsCreated={tagTaxonomy.addOptimisticTags}
          onLogCreated={() => {
            void refreshLogs()
            tagTaxonomy.refresh()
          }}
          allowChat={false}
        />
      </footer>
    </main>
  )
}

export default LogPage
