import type { ReactNode } from 'react'
import './Toolbar.css'

interface ToolbarProps {
  left?: ReactNode
  center?: ReactNode
  right?: ReactNode
  className?: string
  'aria-label'?: string
}

const Toolbar = ({ left, center, right, className = '', 'aria-label': ariaLabel }: ToolbarProps) => {
  return (
    <header className={`app-toolbar ${className}`.trim()} aria-label={ariaLabel}>
      <div className="app-toolbar-section app-toolbar-left">{left}</div>
      {center && <div className="app-toolbar-section app-toolbar-center">{center}</div>}
      <div className="app-toolbar-section app-toolbar-right">{right}</div>
    </header>
  )
}

export default Toolbar
