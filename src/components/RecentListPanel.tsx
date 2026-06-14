import type React from 'react'
import './RecentListPanel.css'

interface RecentListPanelProps<T> {
  label?: string
  title: string
  items: T[]
  loading: boolean
  error: string | null
  bodyRef: React.RefObject<HTMLDivElement | null>
  renderItem: (item: T, index: number) => React.ReactNode
  getItemKey: (item: T, index: number) => React.Key
  onRefresh?: () => void
  onTitleClick?: () => void
  emptyMessage?: string
}

function RecentListPanel<T>({
  label,
  title,
  items,
  loading,
  error,
  bodyRef,
  renderItem,
  getItemKey,
  onRefresh,
  onTitleClick,
  emptyMessage = 'Nothing recent yet.',
}: RecentListPanelProps<T>) {
  return (
    <section className="home-panel recent-list-panel">
      <div className="home-panel-header recent-list-header">
        <div>
          {label && <p>{label}</p>}
          {onTitleClick ? (
            <button className="recent-list-title-button" onClick={onTitleClick} type="button">
              {title}
            </button>
          ) : (
            <h2>{title}</h2>
          )}
        </div>
        {onRefresh && (
          <button className="recent-list-refresh" onClick={onRefresh} disabled={loading}>
            Refresh
          </button>
        )}
      </div>

      <div className="recent-list-body" ref={bodyRef}>
        {loading && items.length === 0 ? (
          <div className="recent-list-state">Loading…</div>
        ) : error ? (
          <div className="recent-list-state recent-list-error">{error}</div>
        ) : items.length === 0 ? (
          <div className="recent-list-state">{emptyMessage}</div>
        ) : (
          <div className="home-list">
            {items.map((item, index) => (
              <div key={getItemKey(item, index)}>{renderItem(item, index)}</div>
            ))}
          </div>
        )}
      </div>
    </section>
  )
}

export default RecentListPanel
