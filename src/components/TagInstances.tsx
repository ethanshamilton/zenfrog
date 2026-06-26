import { useEffect, useState } from 'react'
import { apiService, type TagInstance } from '../services/api'
import './TagInstances.css'

interface TagInstancesProps {
  tag: string
  refreshKey: number
}

const formatInstanceDate = (value: string | null) => {
  if (!value) return 'Unknown date'
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return date.toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
    hour: value.includes('T') ? 'numeric' : undefined,
    minute: value.includes('T') ? '2-digit' : undefined,
  })
}

const formatSourceType = (sourceType: string) => sourceType.replace(/_/g, ' ')

const TagInstances = ({ tag, refreshKey }: TagInstancesProps) => {
  const [instances, setInstances] = useState<TagInstance[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false

    const loadInstances = async () => {
      try {
        setLoading(true)
        setError(null)
        const nextInstances = await apiService.listTagInstances(tag, 20)
        if (!cancelled) {
          setInstances(nextInstances)
        }
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load tag instances')
        }
      } finally {
        if (!cancelled) {
          setLoading(false)
        }
      }
    }

    loadInstances()

    return () => {
      cancelled = true
    }
  }, [tag, refreshKey])

  return (
    <div className="tag-instances">
      <div className="tag-instances-header">
        <div>
          <p className="tag-instances-kicker">Recent uses</p>
          <h2>{tag}</h2>
        </div>
      </div>

      {loading ? (
        <p className="tag-instances-state">Loading instances…</p>
      ) : error ? (
        <p className="tag-instances-state tag-instances-error">{error}</p>
      ) : instances.length === 0 ? (
        <p className="tag-instances-state">No recent uses.</p>
      ) : (
        <div className="tag-instances-list">
          {instances.map((instance) => (
            <article
              className="tag-instance-card"
              key={`${instance.source_type}:${instance.source_id}:${instance.datetime ?? ''}`}
            >
              <header>
                <span>{formatSourceType(instance.source_type)}</span>
                <time>{formatInstanceDate(instance.datetime)}</time>
              </header>
              {instance.title && <strong>{instance.title}</strong>}
              {instance.text && <p>{instance.text}</p>}
            </article>
          ))}
        </div>
      )}
    </div>
  )
}

export default TagInstances
