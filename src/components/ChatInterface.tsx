import { useState, useEffect, useLayoutEffect, useRef } from 'react'
import './ChatInterface.css'
import { apiService } from '../services/api'
import type { Document as CustomDocument, MessageMetadata, SearchIteration, ThreadMessage } from '../types'
import ReactMarkdown from 'react-markdown'

const nativeProviderModels = [
  {
    label: "Anthropic",
    value: "anthropic",
    models: [
      { label: "Claude Fable 5", value: "claude-fable-5" },
      { label: "Claude Opus 4.6", value: "claude-opus-4-6" },
      { label: "Claude Sonnet 5", value: "claude-sonnet-5" },
    ],
  },
  {
    label: "OpenAI",
    value: "openai",
    models: [
      { label: "GPT-5.6 Sol", value: "gpt-5.6-sol"},
      { label: "GPT-5.6 Terra", value: "gpt-5.6-terra"},
      { label: "GPT-5.5", value: "gpt-5.5" }
    ],
  },
  {
    label: "Google",
    value: "google-ai",
    models: [
      { label: "Gemini 3 Pro", value: "gemini-3-pro-preview" }
    ],
  },
]

const openRouterModels = [
  ...nativeProviderModels.flatMap(provider =>
    provider.models.map(model => ({
      label: `${model.label} (${provider.label})`,
      value: `${provider.value === "google-ai" ? "google" : provider.value}/${model.value}`,
    }))
  ),
  { label: "Kimi K3 (Moonshot AI)", value: "moonshotai/kimi-k3" },
]

const providers = [
  ...nativeProviderModels,
  {
    label: "Moonshot AI",
    value: "moonshot",
    models: [{ label: "Kimi K3", value: "kimi-k3" }],
  },
  {
    label: "OpenRouter",
    value: "openrouter",
    models: openRouterModels,
  },
]

interface ModelDisplay {
  model: string
  variant: string
}

const modelDisplayById: Record<string, ModelDisplay> = {
  'claude-fable-5': { model: 'Claude', variant: 'Fable 5' },
  'claude-opus-4-6': { model: 'Claude', variant: 'Opus 4.6' },
  'claude-sonnet-5': { model: 'Claude', variant: 'Sonnet 5' },
  'gpt-5.6-sol': { model: 'GPT-5.6', variant: 'Sol' },
  'gpt-5.6-terra': { model: 'GPT-5.6', variant: 'Terra' },
  'gpt-5.5': { model: 'GPT-5.5', variant: '' },
  'gemini-3-pro-preview': { model: 'Gemini', variant: '3 Pro' },
  'kimi-k3': { model: 'Kimi', variant: 'K3' },
}

interface Message {
  id: string
  text: string
  sender: 'user' | 'assistant'
  timestamp: Date
  metadata?: MessageMetadata | null
}

interface ChatInterfaceProps {
  setDocuments: React.Dispatch<React.SetStateAction<CustomDocument[]>>
  threadId?: string
  initialMessage?: string
  autoSend?: boolean
  launchId?: string
}

const WELCOME_MESSAGE = "Hello! I'm here to help you search through your documents. What would you like to know?"

const makeWelcomeMessage = (): Message => ({
  id: crypto.randomUUID(),
  text: WELCOME_MESSAGE,
  sender: 'assistant',
  timestamp: new Date(),
})

const MessageMetadataView = ({ metadata, onFlipBack }: { metadata: MessageMetadata; onFlipBack: () => void }) => {
  return (
    <div className="metadata-view" onClick={(e) => e.stopPropagation()}>
      <div className="metadata-header">
        <h4>Message Metadata</h4>
        <button className="metadata-flip-back" onClick={onFlipBack}>Back</button>
      </div>

      <section>
        <h5>Model</h5>
        <code>{metadata.model.provider}/{metadata.model.model}</code>
      </section>

      <section>
        <h5>Personality</h5>
        {metadata.personality?.title ? (
          <>
            <div className="metadata-title">{metadata.personality.title}</div>
            {metadata.personality.description && (
              <div className="metadata-muted">{metadata.personality.description}</div>
            )}
            {metadata.personality.prompt && (
              <details>
                <summary>Prompt</summary>
                <pre>{metadata.personality.prompt}</pre>
              </details>
            )}
          </>
        ) : (
          <div className="metadata-muted">Default / none</div>
        )}
      </section>

      <section>
        <h5>Context Entries ({metadata.context_entries.length})</h5>
        {metadata.context_entries.length === 0 ? (
          <div className="metadata-muted">No entries recorded.</div>
        ) : metadata.context_entries.map((entry, idx) => (
          <details key={`${entry.date ?? 'no-date'}-${entry.title}-${idx}`}>
            <summary>
              {entry.date ? `${entry.date} — ` : ''}{entry.title}
              {entry.distance !== null && entry.distance !== undefined ? ` · distance ${entry.distance.toFixed(4)}` : ''}
            </summary>
            <div className="metadata-entry-meta">
              <span>{entry.entry_type}</span>
              <span>{entry.source}</span>
              {entry.entry_id && <span>{entry.entry_id}</span>}
              {entry.tags.length > 0 && <span>{entry.tags.join(' ')}</span>}
            </div>
            {entry.text ? (
              <pre>{entry.text}</pre>
            ) : (
              <div className="metadata-muted">Stored by journal entry reference.</div>
            )}
          </details>
        ))}
      </section>

      <section>
        <h5>Context Logs ({metadata.context_logs.length})</h5>
        {metadata.context_logs.length === 0 ? (
          <div className="metadata-muted">No log retrieval context recorded.</div>
        ) : metadata.context_logs.map((log, idx) => (
          <details key={`${log.log_event_id}-${idx}`}>
            <summary>{log.datetime} · {log.tags.join(' ') || 'untagged'}</summary>
            <div className="metadata-entry-meta">
              <span>{log.source}</span>
              <span>{log.log_event_id}</span>
            </div>
            {log.text ? (
              <pre>{log.text}</pre>
            ) : (
              <div className="metadata-muted">Stored by log event reference.</div>
            )}
          </details>
        ))}
      </section>

      <section>
        <h5>Context Chats ({metadata.context_chats.length})</h5>
        {metadata.context_chats.length === 0 ? (
          <div className="metadata-muted">No chat retrieval context recorded.</div>
        ) : metadata.context_chats.map((chat, idx) => (
          <details key={`${chat.thread_id}-${chat.message_id ?? idx}`}>
            <summary>{chat.role ?? 'message'} · {chat.thread_id.slice(0, 8)}</summary>
            <pre>{chat.content}</pre>
          </details>
        ))}
      </section>

      <section>
        <h5>Retrieval Trace ({metadata.retrieval_trace.length})</h5>
        {metadata.retrieval_trace.length === 0 ? (
          <div className="metadata-muted">No retrieval trace recorded.</div>
        ) : metadata.retrieval_trace.map((iter) => (
          <div key={iter.iteration} className="metadata-trace-item">
            <div className="metadata-trace-header">
              <strong>{iter.iteration}. {iter.tool}</strong>
              <span>{iter.results_count} results, {iter.new_entries_added} new</span>
            </div>
            {iter.query && <code>{iter.query}</code>}
            <p>{iter.reasoning}</p>
          </div>
        ))}
      </section>
    </div>
  )
}

const formatMessageTime = (date: Date) =>
  date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false })

const getModelLeafId = (modelId: string) => {
  const parts = modelId.split('/').filter(Boolean)
  return parts[parts.length - 1] || modelId
}

const getModelDisplay = (modelId: string): ModelDisplay => {
  const leafId = getModelLeafId(modelId)
  return modelDisplayById[leafId] ?? { model: leafId, variant: '' }
}

const ModelPrefixLabel = ({ modelId }: { modelId: string }) => {
  const display = getModelDisplay(modelId)

  return (
    <span className="message-model-id" title={modelId}>
      <span className="message-model-name">{display.model}</span>
      {display.variant && <span className="message-model-variant">{display.variant}</span>}
    </span>
  )
}

const ChatMessageCard = ({ message }: { message: Message }) => {
  const [metadataOpen, setMetadataOpen] = useState(false)
  const hasMetadata = Boolean(message.metadata)
  const fullAssistantModelId = message.metadata?.model.model || 'assistant'

  const toggleMetadata = () => {
    if (!hasMetadata || window.getSelection()?.toString().trim()) return
    setMetadataOpen((isOpen) => !isOpen)
  }

  return (
    <article className={`message-row ${message.sender}-message ${metadataOpen ? 'focused' : ''}`}>
      <button
        type="button"
        className="message-prefix"
        onClick={toggleMetadata}
        disabled={!hasMetadata}
        title={hasMetadata ? 'Toggle message metadata' : undefined}
        aria-expanded={hasMetadata ? metadataOpen : undefined}
      >
        {message.sender === 'user' ? (
          <>
            <span>you</span>
            <time dateTime={message.timestamp.toISOString()}>{formatMessageTime(message.timestamp)}</time>
            <span>&lt;</span>
          </>
        ) : (
          <>
            <time dateTime={message.timestamp.toISOString()}>{formatMessageTime(message.timestamp)}</time>
            <ModelPrefixLabel modelId={fullAssistantModelId} />
            <span>&gt;</span>
          </>
        )}
      </button>
      <div className="message-column">
        {metadataOpen ? (
          <div className="metadata-content">
            <MessageMetadataView metadata={message.metadata!} onFlipBack={() => setMetadataOpen(false)} />
          </div>
        ) : (
          <div
            className={`message-content ${hasMetadata ? 'clickable' : ''}`}
            onClick={toggleMetadata}
            title={hasMetadata ? 'Click to view message metadata' : undefined}
          >
            <ReactMarkdown>{message.text}</ReactMarkdown>
          </div>
        )}
      </div>
    </article>
  )
}

const ChatInterface: React.FC<ChatInterfaceProps> = ({ setDocuments, threadId, initialMessage, autoSend, launchId }) => {

  const [selectedModel, setSelectedModel] = useState({
    provider: "anthropic",
    model: "claude-opus-4-6"
  })

  const [messages, setMessages] = useState<Message[]>([makeWelcomeMessage()])
  const [inputText, setInputText] = useState('')
  const [isLoading, setIsLoading] = useState(false)

  const [currentThreadId, setCurrentThreadId] = useState<string | null>(null)
  const [isThreadSaved, setIsThreadSaved] = useState(false)
  const [searchIterations, setSearchIterations] = useState<SearchIteration[]>([])
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const handledLaunchIdsRef = useRef<Set<string>>(new Set())
  const handledInitialMessageRef = useRef<string | null>(null)

  const loadThread = (threadId: string, threadMessages: ThreadMessage[]) => {
    setCurrentThreadId(threadId)
    setIsThreadSaved(true) // loaded threads are already saved
    const convertedMessages = threadMessages.map((msg) => ({
      id: msg.message_id,
      text: msg.content,
      sender: msg.role === 'user' ? 'user' as const : 'assistant' as const,
      timestamp: new Date(msg.timestamp),
      metadata: msg.metadata ?? null,
    }))
    setMessages(convertedMessages)
  }

  useEffect(() => {
    if (!threadId) return

    let cancelled = false

    const loadThreadMessages = async () => {
      try {
        const threadMessages = await apiService.getThreadMessages(threadId)
        if (!cancelled) {
          loadThread(threadId, threadMessages)
        }
      } catch (error) {
        console.error('Error loading thread messages:', error)
      }
    }

    loadThreadMessages()

    return () => {
      cancelled = true
    }
  }, [threadId])

  const saveChat = async () => {
    const response = await apiService.createThread()
    setCurrentThreadId(response.thread_id)
    setIsThreadSaved(true)
    
    // save all existing messages to the new thread
    try {
      for (const message of messages) {
        if (message.sender !== 'assistant' || message.text !== WELCOME_MESSAGE) {
          await apiService.addMessageToThread(
            response.thread_id,
            message.sender === 'user' ? 'user' : 'assistant',
            message.text,
            message.metadata ?? null,
          )
        }
      }
    } catch (error) {
      console.error('Error saving existing messages to thread:', error)
    }
  }

  const startNewChat = () => {
    setCurrentThreadId(null)
    setIsThreadSaved(false)
    setMessages([makeWelcomeMessage()])
  }

  const sendMessage = async (messageOverride?: string) => {
    const query = (messageOverride ?? inputText).trim()
    if (!query) return
    const userMessage: Message = {
      id: crypto.randomUUID(),
      text: query,
      sender: 'user',
      timestamp: new Date()
    }

    setMessages(prev => [...prev, userMessage])
    setInputText('')
    setIsLoading(true)

    try {
      let similarDocs: CustomDocument[] = []
      let responseText = ""
      let responseMetadata: MessageMetadata | null = null

      // always use streaming agentic flow with fresh retrieval
      setSearchIterations([])
      await apiService.queryJournalStream(
        {
          query,
          top_k: 5,
          provider: selectedModel.provider,
          model: selectedModel.model,
          thread_id: currentThreadId || "",
          message_history: isThreadSaved ? undefined : messages
        },
        (iteration) => {
          setSearchIterations(prev => [...prev, iteration])
        },
        (combinedResponse) => {
          similarDocs = combinedResponse.docs.map((doc, i) => ({
            id: i + 1,
            title: doc.entry.title || `Similar Entry ${i + 1}`,
            content: doc.entry.text || JSON.stringify(doc.entry)
          }))

          setDocuments(similarDocs)
          responseText = combinedResponse.response
          responseMetadata = combinedResponse.message_metadata ?? null

          const botMessage: Message = {
            id: crypto.randomUUID(),
            text: combinedResponse.response,
            sender: 'assistant',
            timestamp: new Date(),
            metadata: responseMetadata,
          }
          setMessages(prev => [...prev, botMessage])
          setSearchIterations([])
        }
      )

      // save messages to thread if we have one and it's saved
      if (currentThreadId && isThreadSaved) {
        try {
          await apiService.addMessageToThread(currentThreadId, 'user', query)
          await apiService.addMessageToThread(currentThreadId, 'assistant', responseText, responseMetadata)
        } catch (error) {
          console.error('Error saving messages to thread:', error)
        }
      }
    } catch (error) {
      console.error('Error querying API:', error)
      const errorMessage: Message = {
        id: crypto.randomUUID(),
        text: 'Sorry, I encountered an error while processing your request. Please make sure the backend is running.',
        sender: 'assistant',
        timestamp: new Date()
      }
      setMessages(prev => [...prev, errorMessage])
    } finally {
      setIsLoading(false)
    }
  }

  useEffect(() => {
    if (!initialMessage) return

    if (launchId) {
      if (handledLaunchIdsRef.current.has(launchId)) return
      handledLaunchIdsRef.current.add(launchId)
    } else {
      const initialMessageKey = `${threadId ?? 'new'}:${initialMessage}:${autoSend ? 'send' : 'draft'}`
      if (handledInitialMessageRef.current === initialMessageKey) return
      handledInitialMessageRef.current = initialMessageKey
    }

    if (autoSend) {
      void sendMessage(initialMessage)
    } else {
      setInputText(initialMessage)
    }
  }, [threadId, initialMessage, autoSend, launchId])

  const handleKeyPress = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      sendMessage()
    }
  }

  useLayoutEffect(() => {
    const textarea = inputRef.current
    if (!textarea) return
    textarea.style.height = 'auto'
    textarea.style.height = `${Math.min(textarea.scrollHeight, 160)}px`
  }, [inputText])

  return (
    <div className="chat-interface">
      <header className="chat-header" aria-label="Chat toolbar">
        <div className="chat-toolbar-group">
          <span className="chat-toolbar-title">Chat</span>
          <label className="chat-model-control">
            <span>Model</span>
            <select
              value={`${selectedModel.provider}:${selectedModel.model}`}
              onChange={e => {
                const [provider, model] = e.target.value.split(":")
                setSelectedModel({ provider, model })
              }}
            >
              {providers.map(provider => (
                <optgroup key={provider.value} label={provider.label}>
                  {provider.models.map(model => (
                    <option key={model.value} value={`${provider.value}:${model.value}`}>
                      {model.label}
                    </option>
                  ))}
                </optgroup>
              ))}
            </select>
          </label>
          <span className="thread-info">
            {currentThreadId ? `Thread ${currentThreadId.slice(0, 8)}` : 'Unsaved thread'}
          </span>
        </div>
        <div className="chat-actions">
          {!isThreadSaved && <button onClick={saveChat}>Save</button>}
          <button onClick={startNewChat}>New Chat</button>
        </div>
      </header>
      
      <section className="chat-messages" aria-label="Chat transcript">
        <div className="chat-transcript">
          {messages.map((message) => <ChatMessageCard key={message.id} message={message} />)}
          {isLoading && (
            <div className="message-row assistant-message loading-row">
              <div className="message-prefix static-prefix">
                <span>··:··</span>
                <ModelPrefixLabel modelId={selectedModel.model} />
                <span>&gt;</span>
              </div>
              <div className="message-column">
                {searchIterations.length > 0 ? (
                  <div className="thinking-panel">
                    {searchIterations.map((iter, idx) => (
                      <div key={idx} className="iteration-card">
                        <div className="iteration-header">
                          <span className="iteration-tool">{iter.tool}</span>
                          {iter.query && <span className="iteration-query">"{iter.query}"</span>}
                          <span className="iteration-meta">
                            {iter.results_count} results, {iter.new_entries_added} new
                          </span>
                        </div>
                        <div className="iteration-reasoning">{iter.reasoning}</div>
                      </div>
                    ))}
                    <div className="typing-indicator"><span></span><span></span><span></span></div>
                  </div>
                ) : (
                  <div className="typing-indicator"><span></span><span></span><span></span></div>
                )}
              </div>
            </div>
          )}
        </div>
      </section>

      <footer className="chat-input" aria-label="Chat composer">
        <div className="input-container">
          <div className="chat-terminal-input">
            <span className="chat-terminal-prompt">you &gt;</span>
            <textarea
              ref={inputRef}
              value={inputText}
              onChange={(e) => setInputText(e.target.value)}
              onKeyDown={handleKeyPress}
              placeholder="Ask your journal..."
              rows={1}
              disabled={isLoading}
            />
          </div>
          <button
            onClick={() => sendMessage()}
            disabled={!inputText.trim() || isLoading}
            className="send-button"
          >
            [Chat]
          </button>
        </div>
      </footer>
    </div>
  )
}

export default ChatInterface
