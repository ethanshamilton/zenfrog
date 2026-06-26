import { useEffect, useState } from 'react'
import Toolbar from '../components/Toolbar'
import TagView from '../components/TagView'
import TaxonomyTree, { type TaxonomyTreeSortMode } from '../components/TaxonomyTree'
import { useTagTaxonomy } from '../hooks/useTagTaxonomy'
import type { TaxonomyTag } from '../services/api'
import './TaxonomyPage.css'

interface TaxonomyPageProps {
  onBackHome: () => void
  onOpenSettings: () => void
}

const TaxonomyPage = ({ onBackHome, onOpenSettings }: TaxonomyPageProps) => {
  const tagTaxonomy = useTagTaxonomy()
  const [selectedTag, setSelectedTag] = useState<string | null>(null)
  const [sortMode, setSortMode] = useState<TaxonomyTreeSortMode>('alphabetical')
  const [instancesRefreshKey, setInstancesRefreshKey] = useState(0)

  useEffect(() => {
    if (tagTaxonomy.tags.length === 0) {
      setSelectedTag(null)
      return
    }

    if (!selectedTag || !tagTaxonomy.tags.some((tag) => tag.tag === selectedTag)) {
      setSelectedTag(tagTaxonomy.tags[0].tag)
    }
  }, [selectedTag, tagTaxonomy.tags])

  const selectedTaxonomyTag = selectedTag
    ? tagTaxonomy.tags.find((tag) => tag.tag === selectedTag) ?? null
    : null

  const handleTagSaved = (tag: TaxonomyTag) => {
    setSelectedTag(tag.tag)
    setInstancesRefreshKey((key) => key + 1)
    tagTaxonomy.refresh()
  }

  const handleTagRenamed = (tag: TaxonomyTag) => {
    setSelectedTag(tag.tag)
    setInstancesRefreshKey((key) => key + 1)
    tagTaxonomy.refresh()
  }

  return (
    <main className="taxonomy-page">
      <Toolbar
        aria-label="Taxonomy toolbar"
        left={(
          <button type="button" onClick={onBackHome} aria-label="Go home">
            Home
          </button>
        )}
        center={<h1>Taxonomy Management</h1>}
        right={(
          <button type="button" onClick={onOpenSettings} aria-label="Open settings">
            Settings
          </button>
        )}
      />

      <section className="taxonomy-page-layout" aria-label="Taxonomy management">
        <aside className="taxonomy-page-tree-panel" aria-label="Taxonomy tags">
          <div className="taxonomy-page-panel-header">
            <h2>Tags</h2>
            <div className="taxonomy-page-tree-controls">
              <label>
                <span>Sort</span>
                <select
                  value={sortMode}
                  onChange={(event) => setSortMode(event.target.value as TaxonomyTreeSortMode)}
                  aria-label="Sort taxonomy tags"
                >
                  <option value="alphabetical">Alphabetical</option>
                  <option value="count">Count</option>
                </select>
              </label>
              <button type="button" onClick={tagTaxonomy.refresh} disabled={tagTaxonomy.loading}>
                Refresh
              </button>
            </div>
          </div>

          {tagTaxonomy.loading ? (
            <p className="taxonomy-page-state">Loading tags…</p>
          ) : tagTaxonomy.error ? (
            <p className="taxonomy-page-state taxonomy-page-error">{tagTaxonomy.error}</p>
          ) : tagTaxonomy.tags.length === 0 ? (
            <p className="taxonomy-page-state">No taxonomy tags yet.</p>
          ) : (
            <TaxonomyTree
              tags={tagTaxonomy.tags}
              selectedTag={selectedTag}
              sortMode={sortMode}
              onSelectTag={setSelectedTag}
            />
          )}
        </aside>

        <section className="taxonomy-page-detail-panel" aria-label="Selected taxonomy tag">
          <TagView
            tag={selectedTaxonomyTag}
            instancesRefreshKey={instancesRefreshKey}
            onTagSaved={handleTagSaved}
            onTagRenamed={handleTagRenamed}
          />
        </section>
      </section>
    </main>
  )
}

export default TaxonomyPage
