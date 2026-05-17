import { useState } from 'react'
import './DocumentViewer.css'
import type { Document } from '../types'

interface DocumentViewerProps {
  documents: Document[]
}

const DocumentViewer = ({ documents }: DocumentViewerProps) => {
  const [selectedDocument, setSelectedDocument] = useState<Document | null>(null)

  return (
    <div className="document-viewer">
      <div className="document-list">
        <h3>Documents</h3>
        <div className="document-items">
          {documents.map((doc) => (
            <div
              key={doc.id}
              className={`document-item ${selectedDocument?.id === doc.id ? 'selected' : ''}`}
              onClick={() => setSelectedDocument(doc)}
            >
              <h4>{doc.title}</h4>
            </div>
          ))}
        </div>
      </div>
      
      <div className="document-content">
        {selectedDocument ? (
          <div>
            <h3>{selectedDocument.title}</h3>
            <div 
              className="content"
              style={{ whiteSpace : 'pre-line' }}
            >
              {selectedDocument.content}
            </div>
          </div>
        ) : (
          <div className="no-document">
            <p>Select a document to view its content</p>
          </div>
        )}
      </div>
    </div>
  )
}

export default DocumentViewer
