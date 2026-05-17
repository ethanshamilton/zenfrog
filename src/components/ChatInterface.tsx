import { useState, useEffect, useRef } from 'react'
import './ChatInterface.css'
import { apiService } from '../services/api'
import type { Document as CustomDocument, MessageMetadata, SearchIteration, ThreadMessage } from '../types'
import ReactMarkdown from 'react-markdown'

const providers = [
  {
    label: "Anthropic",
    value: "anthropic",
    models: [
      { label: "Claude Opus 4.6", value: "claude-opus-4-6" },
      { label: "Claude Sonnet 4.6", value: "claude-sonnet-4-6" },
    ],
  },
  {
    label: "OpenAI",
    value: "openai",
    models: [
      { label: "GPT-5", value: "gpt-5.5" }
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

interface Message {
  id: string
  text: string
  sender: 'user' | 'assistant'
  timestamp: Date
  metadata?: MessageMetadata | null
}

interface ChatInterfaceProps {
  setDocuments: React.Dispatch<React.SetStateAction<CustomDocument[]>>
  onLoadThread?: (threadId: string, messages: ThreadMessage[]) => void
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
              {entry.tags.length > 0 && <span>{entry.tags.join(' ')}</span>}
            </div>
            <pre>{entry.text}</pre>
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

const ChatMessageCard = ({ message }: { message: Message }) => {
  const [flipped, setFlipped] = useState(false)
  const hasMetadata = Boolean(message.metadata)
  const pointerStartRef = useRef<{ x: number; y: number } | null>(null)

  const handlePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    pointerStartRef.current = { x: event.clientX, y: event.clientY }
  }

  const handleFrontClick = (event: React.MouseEvent<HTMLDivElement>) => {
    if (!hasMetadata) return

    const selectedText = window.getSelection()?.toString().trim()
    if (selectedText) return

    const pointerStart = pointerStartRef.current
    if (pointerStart) {
      const moved = Math.hypot(event.clientX - pointerStart.x, event.clientY - pointerStart.y)
      if (moved > 5) return
    }

    setFlipped(true)
  }

  return (
    <div className={`message-card ${flipped ? 'flipped' : ''} ${hasMetadata ? 'has-metadata' : ''}`}>
      {!flipped ? (
        <div
          className={`message-content ${hasMetadata ? 'clickable' : ''}`}
          onPointerDown={handlePointerDown}
          onClick={handleFrontClick}
          title={hasMetadata ? 'Click to view metadata' : undefined}
        >
          <ReactMarkdown>{message.text}</ReactMarkdown>
          {hasMetadata && <div className="metadata-hint">click for metadata</div>}
        </div>
      ) : (
        <div className="message-content metadata-content">
          <MessageMetadataView metadata={message.metadata!} onFlipBack={() => setFlipped(false)} />
        </div>
      )}
    </div>
  )
}

const ChatInterface: React.FC<ChatInterfaceProps> = ({ setDocuments, onLoadThread }) => {

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

  // expose loadThread function to parent
  useEffect(() => {
    if (onLoadThread) {
      // this is a bit hacky but works for now
      ;(window as any).loadThreadIntoChat = loadThread
    }
  }, [onLoadThread])

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

  const sendMessage = async () => {
    if (!inputText.trim()) return

    const query = inputText
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

  const handleKeyPress = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      sendMessage()
    }
  }

  return (
    <div className="chat-interface">
      <div className="chat-header">
        <div className="chat-title">
          <h3>Journal Chat</h3>
          {currentThreadId && (
            <div className="thread-info">
              <span className="thread-id">Thread: {currentThreadId.slice(0, 8)}...</span>
            </div>
          )}
        </div>
        <div className="chat-actions">
          {!isThreadSaved && (
            <button onClick={saveChat} className="save-chat-btn">
              Save Chat
            </button>
          )}
          <button onClick={startNewChat} className="new-chat-btn">
            New Chat
          </button>
        </div>
        <select
          value={`${selectedModel.provider}:${selectedModel.model}`}
          onChange={e => {
            const[provider, model] = e.target.value.split(":");
            setSelectedModel({ provider, model })
          }}
        >
          {providers.map(provider => (
            <optgroup key={provider.value} label={provider.label}>
              {provider.models.map(model => (
                <option
                  key={model.value}
                  value={`${provider.value}:${model.value}`}
                >
                  {model.label}
                </option>
              ))}
            </optgroup>
          ))}
        </select>
      </div>
      
      <div className="chat-messages">
        {messages.map((message) => (
          <div
            key={message.id}
            className={`message ${message.sender === 'user' ? 'user-message' : 'bot-message'}`}
          >
            <ChatMessageCard message={message} />
            <div className="message-timestamp">
              {message.timestamp.toLocaleTimeString()}
            </div>
          </div>
        ))}
        {isLoading && (
          <div className="message bot-message">
            <div className="message-content loading">
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
                  <div className="typing-indicator">
                    <span></span>
                    <span></span>
                    <span></span>
                  </div>
                </div>
              ) : (
                <div className="typing-indicator">
                  <span></span>
                  <span></span>
                  <span></span>
                </div>
              )}
            </div>
          </div>
        )}
      </div>
      
      <div className="chat-input">
        <div className="input-container">
          <textarea
            value={inputText}
            onChange={(e) => setInputText(e.target.value)}
            onKeyPress={handleKeyPress}
            placeholder="Ask a question about your documents..."
            rows={3}
            disabled={isLoading}
          />
          <button
            onClick={sendMessage}
            disabled={!inputText.trim() || isLoading}
            className="send-button"
          >
            Chat
          </button>
        </div>
      </div>
    </div>
  )
}

export default ChatInterface
