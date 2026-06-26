import { useEffect, useState } from 'react'
import { apiService, type TaxonomyTag } from '../services/api'
import './TagEditor.css'

interface TagEditorProps {
  tag: TaxonomyTag
  onSaved: (tag: TaxonomyTag) => void
  onRenamed: (tag: TaxonomyTag) => void
}

const normalizeTag = (value: string) => {
  const trimmed = value.trim()
  if (!trimmed) return null

  const withoutHash = trimmed.replace(/^#+/, '')
  const parts = withoutHash.split('/')
  if (parts.length === 0 || parts.some((part) => part.length === 0 || /\s/.test(part))) {
    return null
  }

  return `#${parts.join('/')}`
}

const normalizeColor = (value: string) => {
  const trimmed = value.trim()
  if (!trimmed) return null
  if (!/^#[0-9A-Fa-f]{6}$/.test(trimmed)) return undefined
  return trimmed.toUpperCase()
}

const TagEditor = ({ tag, onSaved, onRenamed }: TagEditorProps) => {
  const [tagName, setTagName] = useState(tag.tag)
  const [description, setDescription] = useState(tag.description)
  const [color, setColor] = useState(tag.color ?? '')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    setTagName(tag.tag)
    setDescription(tag.description)
    setColor(tag.color ?? '')
    setError(null)
  }, [tag])

  const normalizedTagName = normalizeTag(tagName)
  const normalizedColor = normalizeColor(color)
  const isDirty =
    normalizedTagName !== tag.tag ||
    description !== tag.description ||
    (normalizedColor !== undefined && normalizedColor !== (tag.color ?? null))

  const save = async () => {
    const nextTagName = normalizeTag(tagName)
    if (!nextTagName) {
      setError('Tag name must be a valid path like #Work/EK.')
      return
    }

    const nextColor = normalizeColor(color)
    if (nextColor === undefined) {
      setError('Color must be a hex code like #F54927 or empty.')
      return
    }

    try {
      setSaving(true)
      setError(null)

      let targetTag = tag.tag
      const didRename = nextTagName !== tag.tag

      if (didRename) {
        const renamed = await apiService.renameTaxonomyTag({
          old_tag: tag.tag,
          new_tag: nextTagName,
        })
        targetTag = renamed.tag
      }

      const updated = await apiService.updateTaxonomyTag({
        tag: targetTag,
        description,
        color: nextColor,
      })

      if (didRename) {
        onRenamed(updated)
      } else {
        onSaved(updated)
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save tag')
    } finally {
      setSaving(false)
    }
  }

  return (
    <form
      className="tag-editor"
      onSubmit={(event) => {
        event.preventDefault()
        void save()
      }}
    >
      <div className="tag-editor-header">
        <div>
          <p className="tag-editor-kicker">Tag editor</p>
          <h2>{tag.tag}</h2>
        </div>
        <button type="submit" disabled={saving || !isDirty}>
          {saving ? 'Saving…' : 'Save'}
        </button>
      </div>

      <label className="tag-editor-field">
        <span>Tag name</span>
        <input value={tagName} onChange={(event) => setTagName(event.target.value)} disabled={saving} />
      </label>

      <label className="tag-editor-field">
        <span>Description</span>
        <textarea
          value={description}
          onChange={(event) => setDescription(event.target.value)}
          disabled={saving}
          rows={4}
        />
      </label>

      <label className="tag-editor-field">
        <span>Color</span>
        <input
          value={color}
          onChange={(event) => setColor(event.target.value)}
          disabled={saving}
          placeholder="#F54927 or empty"
        />
      </label>

      <div className="tag-editor-readonly-grid" aria-label="Computed taxonomy relationships">
        <div>
          <span>Broader</span>
          <p>{tag.broader.length > 0 ? tag.broader.join(', ') : '—'}</p>
        </div>
        <div>
          <span>Narrower</span>
          <p>{tag.narrower.length > 0 ? tag.narrower.join(', ') : '—'}</p>
        </div>
      </div>

      {error && <p className="tag-editor-error">{error}</p>}
    </form>
  )
}

export default TagEditor
