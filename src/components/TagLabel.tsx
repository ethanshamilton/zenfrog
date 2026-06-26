import './TagLabel.css'

interface TagLabelProps {
  tag: string
  color?: string | null
  className?: string
}

const TagLabel = ({ tag, color, className = '' }: TagLabelProps) => {
  return (
    <span className={`tag-label ${className}`.trim()} style={color ? { color } : undefined}>
      {tag}
    </span>
  )
}

export default TagLabel
