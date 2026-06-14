import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { KeyboardEvent } from 'react'
import { apiService, type TagSummary } from '../services/api'
import './HomeComposer.css'

interface HomeComposerProps {
  tagSuggestions: TagSummary[]
  onTagsCreated: (tags: string[]) => void
  onLogCreated: () => void
  onOpenChat: (initialMessage: string) => void
}

const MAX_TEXTAREA_HEIGHT = 160

const pad2 = (value: number) => value.toString().padStart(2, '0')

const shortDayCodes = ['Sun', 'M', 'T', 'W', 'R', 'F', 'Sat']

const formatPromptTime = (date: Date) =>
  `${shortDayCodes[date.getDay()]} ${pad2(date.getHours())}:${pad2(date.getMinutes())}`

const formatDateInputValue = (date: Date) =>
  `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`

const formatTimeInputValue = (date: Date) => `${pad2(date.getHours())}:${pad2(date.getMinutes())}`

const makeLocalDateTime = (dateValue: string, timeValue: string) => {
  const [year, month, day] = dateValue.split('-').map(Number)
  const [hour, minute] = timeValue.split(':').map(Number)
  return new Date(year, month - 1, day, hour, minute)
}

interface ActiveTagToken {
  start: number
  end: number
  query: string
}

const findActiveTagToken = (input: string, caretIndex: number): ActiveTagToken | null => {
  const tokenStart = input.lastIndexOf(' ', caretIndex - 1) + 1
  const newlineStart = input.lastIndexOf('\n', caretIndex - 1) + 1
  const start = Math.max(tokenStart, newlineStart)
  const token = input.slice(start, caretIndex)

  if (!token.startsWith('#') || token.length < 1) return null
  if (!/^#[\p{L}\p{N}_/-]*$/u.test(token)) return null

  let end = caretIndex
  while (end < input.length && /[\p{L}\p{N}_/-]/u.test(input[end])) {
    end += 1
  }

  return { start, end, query: token }
}

const parseLogInput = (input: string) => {
  const tags = Array.from(input.matchAll(/(^|\s)(#[\p{L}\p{N}_/-]+)/gu), (match) => match[2])
  const uniqueTags = Array.from(new Set(tags))
  const text = input
    .replace(/(^|\s)#[\p{L}\p{N}_/-]+/gu, '$1')
    .replace(/[ \t]{2,}/g, ' ')
    .replace(/\n{3,}/g, '\n\n')
    .trim()

  return { text, tags: uniqueTags }
}

const HomeComposer = ({
  tagSuggestions,
  onTagsCreated,
  onLogCreated,
  onOpenChat,
}: HomeComposerProps) => {
  const [currentTime, setCurrentTime] = useState(() => new Date())
  const [selectedDateTime, setSelectedDateTime] = useState<Date | null>(null)
  const [isDateTimePickerOpen, setIsDateTimePickerOpen] = useState(false)
  const [timeDraft, setTimeDraft] = useState(() => ({
    hour: formatTimeInputValue(new Date()).slice(0, 2),
    minute: formatTimeInputValue(new Date()).slice(3, 5),
  }))
  const [composerText, setComposerText] = useState('')
  const [caretIndex, setCaretIndex] = useState(0)
  const [selectedSuggestionIndex, setSelectedSuggestionIndex] = useState(0)
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const dateTimePickerRef = useRef<HTMLDivElement>(null)

  const parsedInput = parseLogInput(composerText)
  const chatMessage = composerText.trim()
  const activeTagToken = findActiveTagToken(composerText, caretIndex)
  const matchingTagSuggestions = activeTagToken
    ? tagSuggestions
        .filter((summary) =>
          summary.tag.toLowerCase().startsWith(activeTagToken.query.toLowerCase()) &&
          summary.tag.toLowerCase() !== activeTagToken.query.toLowerCase(),
        )
        .slice(0, 8)
    : []
  const canSubmitLog = parsedInput.text.length > 0 && !submitting
  const canSubmitChat = chatMessage.length > 0 && !submitting

  useEffect(() => {
    const interval = window.setInterval(() => {
      setCurrentTime(new Date())
    }, 30_000)

    return () => window.clearInterval(interval)
  }, [])

  useEffect(() => {
    setSelectedSuggestionIndex(0)
  }, [activeTagToken?.query])

  useEffect(() => {
    if (!isDateTimePickerOpen) return

    const handlePointerDown = (event: PointerEvent) => {
      const picker = dateTimePickerRef.current
      if (!picker || picker.contains(event.target as Node)) return
      setIsDateTimePickerOpen(false)
    }

    document.addEventListener('pointerdown', handlePointerDown)

    return () => document.removeEventListener('pointerdown', handlePointerDown)
  }, [isDateTimePickerOpen])

  useLayoutEffect(() => {
    const textarea = textareaRef.current
    if (!textarea) return

    textarea.style.height = 'auto'
    textarea.style.height = `${Math.min(textarea.scrollHeight, MAX_TEXTAREA_HEIGHT)}px`
  }, [composerText])

  const submitChat = () => {
    const message = composerText.trim()
    if (!message || submitting) return

    setComposerText('')
    setCaretIndex(0)
    onOpenChat(message)
  }

  const submitLog = async () => {
    const { text, tags } = parseLogInput(composerText)
    if (!text || submitting) return

    try {
      setSubmitting(true)
      setError(null)
      await apiService.createLogEvent({
        datetime: (selectedDateTime ?? new Date()).toISOString(),
        text,
        tags,
      })
      setComposerText('')
      setCaretIndex(0)
      setSelectedDateTime(null)
      setIsDateTimePickerOpen(false)
      onTagsCreated(tags)
      onLogCreated()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to create log event')
    } finally {
      setSubmitting(false)
    }
  }

  const updateCaretIndex = () => {
    const textarea = textareaRef.current
    if (!textarea) return
    setCaretIndex(textarea.selectionStart)
  }

  const acceptTagSuggestion = (tag: string) => {
    if (!activeTagToken) return

    const suffix = composerText.slice(activeTagToken.end)
    const separator = suffix.startsWith(' ') || suffix.startsWith('\n') ? '' : ' '
    const nextText = `${composerText.slice(0, activeTagToken.start)}${tag}${separator}${suffix}`
    const nextCaretIndex = activeTagToken.start + tag.length + separator.length
    setComposerText(nextText)
    setCaretIndex(nextCaretIndex)
    setSelectedSuggestionIndex(0)

    window.requestAnimationFrame(() => {
      textareaRef.current?.setSelectionRange(nextCaretIndex, nextCaretIndex)
      textareaRef.current?.focus()
    })
  }

  const handleKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (matchingTagSuggestions.length > 0) {
      if (event.key === 'ArrowDown') {
        event.preventDefault()
        setSelectedSuggestionIndex((index) => (index + 1) % matchingTagSuggestions.length)
        return
      }

      if (event.key === 'ArrowUp') {
        event.preventDefault()
        setSelectedSuggestionIndex((index) =>
          index === 0 ? matchingTagSuggestions.length - 1 : index - 1,
        )
        return
      }

      if (event.key === 'Tab' || (event.key === 'Enter' && !event.ctrlKey && !event.metaKey)) {
        event.preventDefault()
        const selectedSuggestion = matchingTagSuggestions[selectedSuggestionIndex]
        if (selectedSuggestion) {
          acceptTagSuggestion(selectedSuggestion.tag)
        }
        return
      }

      if (event.key === 'Escape') {
        event.preventDefault()
        setCaretIndex(-1)
        return
      }
    }

    if (event.key !== 'Enter') return

    if (event.ctrlKey || event.metaKey) {
      event.preventDefault()
      submitChat()
      return
    }

    if (!event.shiftKey) {
      event.preventDefault()
      void submitLog()
    }
  }

  const logDateTime = selectedDateTime ?? currentTime

  const handleDateChange = (dateValue: string) => {
    setSelectedDateTime(makeLocalDateTime(dateValue, formatTimeInputValue(logDateTime)))
  }

  const openDateTimePicker = () => {
    setTimeDraft({
      hour: pad2(logDateTime.getHours()),
      minute: pad2(logDateTime.getMinutes()),
    })
    setIsDateTimePickerOpen(true)
  }

  const closeOrOpenDateTimePicker = () => {
    if (isDateTimePickerOpen) {
      setIsDateTimePickerOpen(false)
    } else {
      openDateTimePicker()
    }
  }

  const setLogTime = (hourValue: string, minuteValue: string) => {
    setSelectedDateTime(makeLocalDateTime(formatDateInputValue(logDateTime), `${hourValue}:${minuteValue}`))
  }

  const handleHourChange = (hourValue: string) => {
    if (!/^\d{0,2}$/.test(hourValue)) return
    setTimeDraft((draft) => ({ ...draft, hour: hourValue }))

    if (hourValue === '') return
    const hour = Number(hourValue)
    if (hour >= 0 && hour <= 23) {
      setLogTime(pad2(hour), timeDraft.minute || pad2(logDateTime.getMinutes()))
    }
  }

  const handleMinuteChange = (minuteValue: string) => {
    if (!/^\d{0,2}$/.test(minuteValue)) return
    setTimeDraft((draft) => ({ ...draft, minute: minuteValue }))

    if (minuteValue === '') return
    const minute = Number(minuteValue)
    if (minute >= 0 && minute <= 59) {
      setLogTime(timeDraft.hour || pad2(logDateTime.getHours()), pad2(minute))
    }
  }

  const normalizeTimeDraft = () => {
    const hour = timeDraft.hour === '' ? logDateTime.getHours() : Number(timeDraft.hour)
    const minute = timeDraft.minute === '' ? logDateTime.getMinutes() : Number(timeDraft.minute)
    const normalizedHour = pad2(Math.min(23, Math.max(0, hour)))
    const normalizedMinute = pad2(Math.min(59, Math.max(0, minute)))
    setTimeDraft({ hour: normalizedHour, minute: normalizedMinute })
    setLogTime(normalizedHour, normalizedMinute)
  }

  const resetToNow = () => {
    const now = new Date()
    setSelectedDateTime(null)
    setCurrentTime(now)
    setTimeDraft({ hour: pad2(now.getHours()), minute: pad2(now.getMinutes()) })
  }

  return (
    <section className="home-composer" aria-label="Composer">
      <div className="home-composer-row">
        <div className="home-terminal-input">
          <div className="home-terminal-prompt-wrap" ref={dateTimePickerRef}>
            <button
              type="button"
              className="home-terminal-prompt"
              onClick={closeOrOpenDateTimePicker}
              aria-label="Set log date and time"
              title="Set log date/time"
            >
              {formatPromptTime(logDateTime)} &gt;
            </button>
            {isDateTimePickerOpen && (
              <div className="home-datetime-popover">
                <label>
                  Day
                  <input
                    type="date"
                    value={formatDateInputValue(logDateTime)}
                    onChange={(event) => handleDateChange(event.target.value)}
                  />
                </label>
                <label>
                  Time
                  <div className="home-time-fields">
                    <input
                      aria-label="Log hour, 24-hour time"
                      inputMode="numeric"
                      pattern="[0-9]*"
                      value={timeDraft.hour}
                      onChange={(event) => handleHourChange(event.target.value)}
                      onBlur={normalizeTimeDraft}
                      maxLength={2}
                    />
                    <span>:</span>
                    <input
                      aria-label="Log minute"
                      inputMode="numeric"
                      pattern="[0-9]*"
                      value={timeDraft.minute}
                      onChange={(event) => handleMinuteChange(event.target.value)}
                      onBlur={normalizeTimeDraft}
                      maxLength={2}
                    />
                  </div>
                </label>
                <button type="button" onClick={resetToNow}>
                  Now
                </button>
              </div>
            )}
          </div>
          <textarea
            ref={textareaRef}
            value={composerText}
            onChange={(event) => {
              setComposerText(event.target.value)
              setCaretIndex(event.target.selectionStart)
            }}
            onClick={updateCaretIndex}
            onKeyUp={updateCaretIndex}
            onSelect={updateCaretIndex}
            onKeyDown={handleKeyDown}
            placeholder="ENTER writes a new log event. CTRL+ENTER starts a new chat."
            rows={1}
            disabled={submitting}
          />
          {matchingTagSuggestions.length > 0 && (
            <div className="home-tag-suggestions">
              {matchingTagSuggestions.map((summary, index) => (
                <button
                  key={summary.tag}
                  type="button"
                  className={index === selectedSuggestionIndex ? 'active' : ''}
                  onMouseDown={(event) => {
                    event.preventDefault()
                    acceptTagSuggestion(summary.tag)
                  }}
                >
                  <span>{summary.tag}</span>
                  <small>{summary.count}</small>
                </button>
              ))}
            </div>
          )}
        </div>
        <div className="home-composer-actions">
          <button onClick={() => void submitLog()} disabled={!canSubmitLog} aria-label="Create log event">
            [Log]
          </button>
          <button onClick={submitChat} disabled={!canSubmitChat} aria-label="Start chat from composer text">
            [Chat]
          </button>
        </div>
      </div>
      {error && <div className="home-composer-error">{error}</div>}
    </section>
  )
}

export default HomeComposer
