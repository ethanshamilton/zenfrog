import type { TaxonomyTag } from '../services/api'

export const normalizeTag = (tag: string) => {
  const parts = tag
    .trim()
    .replace(/^#+/, '')
    .split('/')
    .map((part) => part.trim())
    .filter(Boolean)

  return parts.length > 0 ? `#${parts.join('/')}` : null
}

export const uniqueNormalizedTags = (tags: string[]) =>
  Array.from(new Set(tags.map(normalizeTag).filter((tag): tag is string => Boolean(tag))))

export const parentTag = (tag: string) => {
  const normalized = normalizeTag(tag)
  if (!normalized) return null
  const parts = normalized.replace(/^#/, '').split('/')
  if (parts.length <= 1) return null
  return `#${parts.slice(0, -1).join('/')}`
}

export const createTaxonomyMap = (tags: TaxonomyTag[]) => new Map(tags.map((tag) => [tag.tag, tag]))

export const getEffectiveTagColor = (tag: string, taxonomyByTag: Map<string, TaxonomyTag>) => {
  let current = normalizeTag(tag)

  while (current) {
    const taxonomyTag = taxonomyByTag.get(current)
    if (taxonomyTag?.color) return taxonomyTag.color
    current = parentTag(current)
  }

  return undefined
}
