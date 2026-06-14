import { useState, useEffect, useRef } from 'react'
import './App.css'
import Sidebar from './components/Sidebar'
import ChatInterface from './components/ChatInterface'
import LoadingScreen from './components/LoadingScreen'
import HomePage from './pages/HomePage'
import LogPage from './pages/LogPage'
import NotImplementedPage from './components/NotImplementedPage'
import { apiService } from './services/api'
import type { Document } from './types'

export type AppPage =
  | { name: 'home' }
  | { name: 'chat'; threadId?: string; initialMessage?: string; autoSend?: boolean; launchId?: string }
  | { name: 'logs'; focusedLogEventId?: string }
  | { name: 'settings' }

function App() {
  const [documents, setDocuments] = useState<Document[]>([])
  const [page, setPage] = useState<AppPage>({ name: 'home' })
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

  const renderPage = () => {
    switch (page.name) {
      case 'home':
        return (
          <HomePage
            onOpenChat={(chatPage = {}) => setPage({ name: 'chat', ...chatPage })}
            onOpenLogs={(focusedLogEventId) => setPage({ name: 'logs', focusedLogEventId })}
            onOpenSettings={() => setPage({ name: 'settings' })}
          />
        )
      case 'chat':
        return (
          <div className="app-container">
            <Sidebar
              documents={documents}
              onBackHome={() => setPage({ name: 'home' })}
              onLoadThread={(threadId) => setPage({ name: 'chat', threadId })}
            />
            <ChatInterface
              setDocuments={setDocuments}
              threadId={page.threadId}
              initialMessage={page.initialMessage}
              autoSend={page.autoSend}
              launchId={page.launchId}
            />
          </div>
        )
      case 'logs':
        return (
          <LogPage
            focusedLogEventId={page.focusedLogEventId}
            onBackHome={() => setPage({ name: 'home' })}
            onOpenSettings={() => setPage({ name: 'settings' })}
          />
        )
      case 'settings':
        return (
          <NotImplementedPage
            title="Settings"
            description="App configuration will live here."
            onBackHome={() => setPage({ name: 'home' })}
          />
        )
    }
  }

  if (!isBackendReady) {
    return <LoadingScreen />
  }

  return <div className="app">{renderPage()}</div>
}

export default App
