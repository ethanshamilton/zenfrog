import { useEffect, useMemo, useState } from 'react'
import { apiService } from '../services/api'
import { normalizeTag, uniqueNormalizedTags } from '../utils/tags'

export function useResolvedTagColors(tags: string[]) {
  const normalizedTags = useMemo(() => uniqueNormalizedTags(tags).sort(), [tags])
  const tagKey = normalizedTags.join('\u001f')
  const [colors, setColors] = useState<Record<string, string | null>>({})

  useEffect(() => {
    let cancelled = false

    const loadColors = async () => {
      if (normalizedTags.length === 0) {
        setColors({})
        return
      }

      try {
        const nextColors = await apiService.resolveTagColors(normalizedTags)
        if (!cancelled) {
          setColors(nextColors)
        }
      } catch {
        if (!cancelled) {
          setColors({})
        }
      }
    }

    loadColors()

    return () => {
      cancelled = true
    }
  }, [tagKey])

  return (tag: string) => {
    const normalized = normalizeTag(tag)
    return normalized ? colors[normalized] ?? undefined : undefined
  }
}
