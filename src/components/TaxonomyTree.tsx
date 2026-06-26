import type { TaxonomyTag } from '../services/api'
import TagLabel from './TagLabel'
import { createTaxonomyMap, getEffectiveTagColor } from '../utils/tags'
import './TaxonomyTree.css'

export type TaxonomyTreeSortMode = 'alphabetical' | 'count'

interface TaxonomyTreeProps {
  tags: TaxonomyTag[]
  selectedTag: string | null
  sortMode: TaxonomyTreeSortMode
  onSelectTag: (tag: string) => void
}

interface TaxonomyTreeNode {
  tag: TaxonomyTag
  children: TaxonomyTreeNode[]
}

const parentPath = (tag: string) => {
  const parts = tag.replace(/^#/, '').split('/').filter(Boolean)
  if (parts.length <= 1) return null
  return `#${parts.slice(0, -1).join('/')}`
}

const compareNodes = (sortMode: TaxonomyTreeSortMode) => (
  a: TaxonomyTreeNode,
  b: TaxonomyTreeNode,
) => {
  if (sortMode === 'count') {
    return b.tag.count - a.tag.count || a.tag.tag.localeCompare(b.tag.tag)
  }

  return a.tag.tag.localeCompare(b.tag.tag)
}

const buildTree = (tags: TaxonomyTag[], sortMode: TaxonomyTreeSortMode): TaxonomyTreeNode[] => {
  const sortedTags = [...tags].sort((a, b) => a.tag.localeCompare(b.tag))
  const byTag = new Map<string, TaxonomyTreeNode>(
    sortedTags.map((tag) => [tag.tag, { tag, children: [] }]),
  )
  const roots: TaxonomyTreeNode[] = []

  for (const node of byTag.values()) {
    const parent = parentPath(node.tag.tag)
    const parentNode = parent ? byTag.get(parent) : null

    if (parentNode) {
      parentNode.children.push(node)
    } else {
      roots.push(node)
    }
  }

  const sortNodes = (nodes: TaxonomyTreeNode[]) => {
    nodes.sort(compareNodes(sortMode))
    nodes.forEach((node) => sortNodes(node.children))
  }

  sortNodes(roots)
  return roots
}

const renderNode = (
  node: TaxonomyTreeNode,
  selectedTag: string | null,
  onSelectTag: (tag: string) => void,
  taxonomyByTag: Map<string, TaxonomyTag>,
  depth = 0,
) => {
  const isSelected = node.tag.tag === selectedTag

  return (
    <li key={node.tag.tag}>
      <button
        type="button"
        className={`taxonomy-tree-node-button ${isSelected ? 'selected' : ''}`}
        style={{ paddingLeft: `${0.75 + depth * 1.25}rem` }}
        onClick={() => onSelectTag(node.tag.tag)}
        aria-current={isSelected ? 'true' : undefined}
      >
        <TagLabel tag={node.tag.tag} color={getEffectiveTagColor(node.tag.tag, taxonomyByTag)} />
        <small>{node.tag.count}</small>
      </button>

      {node.children.length > 0 && (
        <ul>
          {node.children.map((child) => renderNode(child, selectedTag, onSelectTag, taxonomyByTag, depth + 1))}
        </ul>
      )}
    </li>
  )
}

const TaxonomyTree = ({ tags, selectedTag, sortMode, onSelectTag }: TaxonomyTreeProps) => {
  const roots = buildTree(tags, sortMode)
  const taxonomyByTag = createTaxonomyMap(tags)

  return (
    <nav className="taxonomy-tree" aria-label="Taxonomy tree">
      <ul>{roots.map((node) => renderNode(node, selectedTag, onSelectTag, taxonomyByTag))}</ul>
    </nav>
  )
}

export default TaxonomyTree
