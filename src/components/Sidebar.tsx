import { useState } from 'react'
import './Sidebar.css'
import DocumentViewer from './DocumentViewer'
import ChatViewer from './ChatViewer'
import type { Document, ThreadMessage } from '../types'

interface SidebarProps {
  documents: Document[]
  onLoadThread: (threadId: string, messages: ThreadMessage[]) => void
}

type TabType = 'documents' | 'chats'

const Sidebar = ({ documents, onLoadThread }: SidebarProps) => {
  const [activeTab, setActiveTab] = useState<TabType>('documents')

  return (
    <div className="sidebar">
      <div className="sidebar-tabs">
        <button
          className={`tab ${activeTab === 'documents' ? 'active' : ''}`}
          onClick={() => setActiveTab('documents')}
        >
          Documents
        </button>
        <button
          className={`tab ${activeTab === 'chats' ? 'active' : ''}`}
          onClick={() => setActiveTab('chats')}
        >
          Chats
        </button>
      </div>
      
      <div className="sidebar-content">
        {activeTab === 'documents' ? (
          <DocumentViewer documents={documents} />
        ) : (
          <ChatViewer onLoadThread={onLoadThread} />
        )}
      </div>
    </div>
  )
}

export default Sidebar
