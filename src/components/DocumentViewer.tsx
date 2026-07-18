import { useState } from 'react'
import './DocumentViewer.css'
import type { Document } from '../types'

interface DocumentViewerProps {
  documents: Document[]
}

const DocumentViewer = ({ documents }: DocumentViewerProps) => {
  const [expandedDocumentId, setExpandedDocumentId] = useState<number | null>(null)

  return (
    <div className="document-viewer">
      <div className="document-list">
        <h3>Documents</h3>
        {documents.length === 0 ? (
          <div className="no-document">
            <p>No retrieved documents yet.</p>
          </div>
        ) : (
          <div className="document-items">
            {documents.map((doc) => {
              const isExpanded = expandedDocumentId === doc.id

              return (
                <article
                  key={doc.id}
                  className={`document-item ${isExpanded ? 'expanded' : ''}`}
                >
                  <button
                    type="button"
                    className="document-item-toggle"
                    onClick={() => setExpandedDocumentId(isExpanded ? null : doc.id)}
                    aria-expanded={isExpanded}
                  >
                    <h4>{doc.title}</h4>
                    <span className="document-item-caret" aria-hidden="true">
                      {isExpanded ? '▾' : '▸'}
                    </span>
                  </button>
                  {isExpanded && (
                    <div className="document-item-content">
                      {doc.content}
                    </div>
                  )}
                </article>
              )
            })}
          </div>
        )}
      </div>
    </div>
  )
}

export default DocumentViewer
