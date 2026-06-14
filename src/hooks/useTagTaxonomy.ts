import { useCallback, useEffect, useState } from 'react'
import { apiService, type TagSummary } from '../services/api'

export function useTagTaxonomy() {
  const [tags, setTags] = useState<TagSummary[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [refreshCount, setRefreshCount] = useState(0)

  useEffect(() => {
    let cancelled = false

    const loadTags = async () => {
      try {
        setLoading(true)
        setError(null)
        const nextTags = await apiService.listTags()
        if (!cancelled) {
          setTags(nextTags)
        }
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load tags')
        }
      } finally {
        if (!cancelled) {
          setLoading(false)
        }
      }
    }

    loadTags()

    return () => {
      cancelled = true
    }
  }, [refreshCount])

  const refresh = useCallback(() => {
    setRefreshCount((count) => count + 1)
  }, [])

  const addOptimisticTags = useCallback((newTags: string[]) => {
    setTags((currentTags) => {
      const byTag = new Map(currentTags.map((summary) => [summary.tag, summary]))

      for (const tag of newTags) {
        const existing = byTag.get(tag)
        byTag.set(tag, {
          tag,
          count: existing ? existing.count + 1 : 1,
        })
      }

      return Array.from(byTag.values()).sort((a, b) =>
        b.count - a.count || a.tag.localeCompare(b.tag),
      )
    })
  }, [])

  return {
    tags,
    loading,
    error,
    refresh,
    addOptimisticTags,
  }
}
