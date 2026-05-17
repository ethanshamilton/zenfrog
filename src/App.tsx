import { useState, useEffect, useRef } from 'react'
import './App.css'
import Sidebar from './components/Sidebar'
import ChatInterface from './components/ChatInterface'
import LoadingScreen from './components/LoadingScreen'
import { apiService } from './services/api'
import type { Document, ThreadMessage } from './types'

function App() {
  const [documents, setDocuments] = useState<Document[]>([])
  const [isBackendReady, setIsBackendReady] = useState(false)
  const intervalRef = useRef<number | null>(null)
  const isReadyRef = useRef(false)

  // Start polling for backend status
  useEffect(() => {
    const checkBackendStatus = async () => {
      if (isReadyRef.current) return

      try {
        const response = await apiService.checkStatus()
        if (response.status === 'ready') {
          isReadyRef.current = true
          setIsBackendReady(true)
        }
      } catch {
        // Backend not up yet, continue polling
      }
    }

    if (!isReadyRef.current) {
      intervalRef.current = window.setInterval(checkBackendStatus, 2000)
      checkBackendStatus()
    }

    return () => {
      if (intervalRef.current !== null) {
        clearInterval(intervalRef.current)
        intervalRef.current = null
      }
    }
  }, [])

  // Ensure polling stops when backend is ready
  useEffect(() => {
    if (isBackendReady && intervalRef.current !== null) {
      clearInterval(intervalRef.current)
      intervalRef.current = null
    }
  }, [isBackendReady])

  const handleLoadThread = (threadId: string, messages: ThreadMessage[]) => {
    // use the exposed function from ChatInterface
    if ((window as any).loadThreadIntoChat) {
      ;(window as any).loadThreadIntoChat(threadId, messages)
    }
  }

  if (!isBackendReady) {
    return <LoadingScreen />
  }

  return (
    <div className="app">
      <div className="app-container">
        <Sidebar documents={documents} onLoadThread={handleLoadThread} />
        <ChatInterface setDocuments={setDocuments} onLoadThread={handleLoadThread} />
      </div>
    </div>
  )
}

export default App
