import { useCallback, useEffect, useRef, useState } from 'react'
import type { RefObject } from 'react'

interface UseMeasuredRecentListArgs<T> {
  fetchItems: (limit: number) => Promise<T[]>
  estimateItemHeight: number
  overscan?: number
  minLimit?: number
}

interface UseMeasuredRecentListResult<T> {
  items: T[]
  loading: boolean
  error: string | null
  limit: number
  refresh: () => void
  bodyRef: RefObject<HTMLDivElement | null>
}

export function useMeasuredRecentList<T>({
  fetchItems,
  estimateItemHeight,
  overscan = 3,
  minLimit = 1,
}: UseMeasuredRecentListArgs<T>): UseMeasuredRecentListResult<T> {
  const bodyRef = useRef<HTMLDivElement | null>(null)
  const [limit, setLimit] = useState(minLimit)
  const [items, setItems] = useState<T[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [refreshCount, setRefreshCount] = useState(0)

  useEffect(() => {
    const element = bodyRef.current
    if (!element) return

    const updateLimit = (height: number) => {
      const nextLimit = Math.max(
        minLimit,
        Math.ceil(height / estimateItemHeight) + overscan,
      )
      setLimit((currentLimit) => (currentLimit === nextLimit ? currentLimit : nextLimit))
    }

    updateLimit(element.getBoundingClientRect().height)

    const resizeObserver = new ResizeObserver((entries) => {
      const entry = entries[0]
      if (!entry) return
      updateLimit(entry.contentRect.height)
    })

    resizeObserver.observe(element)

    return () => resizeObserver.disconnect()
  }, [estimateItemHeight, minLimit, overscan])

  useEffect(() => {
    let cancelled = false

    const loadItems = async () => {
      try {
        setLoading(true)
        setError(null)
        const nextItems = await fetchItems(limit)
        if (!cancelled) {
          setItems(nextItems)
        }
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load recent items')
        }
      } finally {
        if (!cancelled) {
          setLoading(false)
        }
      }
    }

    loadItems()

    return () => {
      cancelled = true
    }
  }, [fetchItems, limit, refreshCount])

  const refresh = useCallback(() => {
    setRefreshCount((count) => count + 1)
  }, [])

  return {
    items,
    loading,
    error,
    limit,
    refresh,
    bodyRef,
  }
}
