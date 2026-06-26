import { useCallback, useEffect, useState } from 'react'
import { apiService, type TaxonomyTag } from '../services/api'

const normalizeTag = (tag: string) => {
  const parts = tag
    .trim()
    .replace(/^#+/, '')
    .split('/')
    .map((part) => part.trim())
    .filter(Boolean)

  return parts.length > 0 ? `#${parts.join('/')}` : null
}

const sortTags = (tags: TaxonomyTag[]) =>
  [...tags].sort((a, b) => b.count - a.count || a.tag.localeCompare(b.tag))

const makeOptimisticTaxonomyTag = (tag: string, existing?: TaxonomyTag): TaxonomyTag => ({
  tag,
  description: existing?.description ?? '',
  color: existing?.color ?? null,
  broader: existing?.broader ?? [],
  narrower: existing?.narrower ?? [],
  count: (existing?.count ?? 0) + 1,
})

export function useTagTaxonomy() {
  const [tags, setTags] = useState<TaxonomyTag[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [refreshCount, setRefreshCount] = useState(0)

  useEffect(() => {
    let cancelled = false

    const loadTags = async () => {
      try {
        setLoading(true)
        setError(null)
        const nextTags = await apiService.listTaxonomyTags()
        if (!cancelled) {
          setTags(sortTags(nextTags))
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
      const byTag = new Map(currentTags.map((taxonomyTag) => [taxonomyTag.tag, taxonomyTag]))

      for (const rawTag of newTags) {
        const tag = normalizeTag(rawTag)
        if (!tag) continue

        byTag.set(tag, makeOptimisticTaxonomyTag(tag, byTag.get(tag)))
      }

      return sortTags(Array.from(byTag.values()))
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
