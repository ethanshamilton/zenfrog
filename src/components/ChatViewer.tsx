import { useState, useEffect } from 'react'
import './ChatViewer.css'
import { apiService } from '../services/api'
import type { Thread, ThreadMessage } from '../types'

interface ChatViewerProps {
  onLoadThread: (threadId: string, messages: ThreadMessage[]) => void
}

const ChatViewer = ({ onLoadThread }: ChatViewerProps) => {
  const [threads, setThreads] = useState<Thread[]>([])
  const [loading, setLoading] = useState(false)
  const [editingThreadId, setEditingThreadId] = useState<string | null>(null)
  const [editTitle, setEditTitle] = useState('')
  const [generatingThreadId, setGeneratingThreadId] = useState<string | null>(null)

  useEffect(() => {
    loadThreads()
  }, [])

  const loadThreads = async () => {
    try {
      setLoading(true)
      const threadsData = await apiService.getThreads()
      setThreads(threadsData)
    } catch (error) {
      console.error('Error loading threads:', error)
    } finally {
      setLoading(false)
    }
  }

  const handleLoadThread = async (threadId: string, existingMessages: ThreadMessage[]) => {
    try {
      // if we don't have messages yet, fetch them
      let messages = existingMessages
      if (messages.length === 0) {
        messages = await apiService.getThreadMessages(threadId)
      }
      onLoadThread(threadId, messages)
    } catch (error) {
      console.error('Error loading thread messages:', error)
    }
  }

  const handleDeleteThread = async (threadId: string) => {
    try {
      await apiService.deleteThread(threadId)
      setThreads(threads.filter(t => t.thread_id !== threadId))
    } catch (error) {
      console.error('Error deleting thread:', error)
    }
  }

  const handleStartEdit = (thread: Thread) => {
    setEditingThreadId(thread.thread_id)
    setEditTitle(thread.title)
  }

  const handleSaveEdit = async () => {
    if (!editingThreadId || !editTitle.trim()) {
      handleCancelEdit()
      return
    }
    
    try {
      await apiService.updateThreadTitle(editingThreadId, editTitle.trim())
      setThreads(threads.map(t => 
        t.thread_id === editingThreadId 
          ? { ...t, title: editTitle.trim() }
          : t
      ))
      setEditingThreadId(null)
      setEditTitle('')
    } catch (error) {
      console.error('Error updating thread title:', error)
      handleCancelEdit()
    }
  }

  const handleCancelEdit = () => {
    setEditingThreadId(null)
    setEditTitle('')
  }

  const handleGenerateTitle = async (threadId: string) => {
    try {
      setGeneratingThreadId(threadId)
      const { title } = await apiService.generateThreadTitle(threadId)
      setThreads(prev => prev.map(t =>
        t.thread_id === threadId ? { ...t, title } : t
      ))
    } catch (error) {
      console.error('Error generating thread title:', error)
    } finally {
      setGeneratingThreadId(null)
    }
  }

  return (
    <div className="chat-viewer">
      <div className="thread-list-header">
        <h3>Chat Threads</h3>
        <button onClick={loadThreads} disabled={loading}>
          {loading ? 'Loading...' : 'Refresh'}
        </button>
      </div>
      <div className="thread-items">
        {threads.map((thread) => (
          <div
            key={thread.thread_id}
            className="thread-item"
            onClick={() => handleLoadThread(thread.thread_id, [])}
            onDoubleClick={() => handleStartEdit(thread)}
          >
            <div className="thread-header">
              {editingThreadId === thread.thread_id ? (
                <input
                  type="text"
                  value={editTitle}
                  onChange={(e) => setEditTitle(e.target.value)}
                  onBlur={handleSaveEdit}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') handleSaveEdit()
                    if (e.key === 'Escape') handleCancelEdit()
                  }}
                  className="edit-title-input"
                  autoFocus
                />
              ) : (
                <h4>{thread.title}</h4>
              )}
              <button
                className="generate-title"
                title="Generate title from chat"
                disabled={generatingThreadId === thread.thread_id}
                onClick={(e) => {
                  e.stopPropagation()
                  handleGenerateTitle(thread.thread_id)
                }}
              >
                {generatingThreadId === thread.thread_id ? '…' : '✎'}
              </button>
              <button
                className="delete-thread"
                onClick={(e) => {
                  e.stopPropagation()
                  handleDeleteThread(thread.thread_id)
                }}
              >
                ×
              </button>
            </div>
            <div className="thread-meta">
              <span>{new Date(thread.updated_at).toLocaleDateString()}</span>
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}

export default ChatViewer
