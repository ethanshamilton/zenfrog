import type { TaxonomyTag } from '../services/api'
import TagEditor from './TagEditor'
import TagInstances from './TagInstances'
import './TagView.css'

interface TagViewProps {
  tag: TaxonomyTag | null
  instancesRefreshKey: number
  onTagSaved: (tag: TaxonomyTag) => void
  onTagRenamed: (tag: TaxonomyTag) => void
}

const TagView = ({ tag, instancesRefreshKey, onTagSaved, onTagRenamed }: TagViewProps) => {
  if (!tag) {
    return <p className="tag-view-empty">Select a tag to view details.</p>
  }

  return (
    <div className="tag-view">
      <section className="tag-view-section" aria-label="Tag editor">
        <TagEditor tag={tag} onSaved={onTagSaved} onRenamed={onTagRenamed} />
      </section>
      <section className="tag-view-section" aria-label="Recent tag instances">
        <TagInstances tag={tag.tag} refreshKey={instancesRefreshKey} />
      </section>
    </div>
  )
}

export default TagView
