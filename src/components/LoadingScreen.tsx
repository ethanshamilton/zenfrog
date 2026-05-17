import { useEffect, useRef, useState } from 'react'
import './LoadingScreen.css'

// Character set: alphanumeric
const CHARACTERS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789'

interface Stream {
  y: number
  x: number
  speed: number
  fontSize: number
  leadTag: string
  tailChars: string[]
  tailLength: number
  whiskerChars: string[]
  whiskerLength: number
}

const LoadingScreen: React.FC = () => {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const [tagData, setTagData] = useState<Record<string, number> | null>(null)

  // Fetch tags from static file on mount
  useEffect(() => {
    fetch('/tags.json')
      .then((res) => {
        if (!res.ok) throw new Error('Not found')
        return res.json()
      })
      .then((data) => setTagData(data))
      .catch(() => {
        // No tags file — fallback to random characters handled in animation
      })
  }, [])

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return

    const ctx = canvas.getContext('2d')
    if (!ctx) return

    // Derive tag constants from fetched data (if available)
    const TAGS = tagData ? Object.keys(tagData) : null
    const MAX_FREQUENCY = tagData ? Math.max(...Object.values(tagData)) : 1
    const MIN_FREQUENCY = tagData ? Math.min(...Object.values(tagData)) : 1

    // Set canvas to full viewport
    const resize = () => {
      canvas.width = window.innerWidth
      canvas.height = window.innerHeight
    }
    resize()
    window.addEventListener('resize', resize)

    const minFontSize = 10
    const maxFontSize = 30
    const minSpeed = 2
    const maxSpeed = 5

    // Map speed to font size (faster = bigger for parallax effect)
    const speedToFontSize = (speed: number) => {
      const ratio = (speed - minSpeed) / (maxSpeed - minSpeed)
      return Math.round(minFontSize + ratio * (maxFontSize - minFontSize))
    }

    // Helper to get random tag or random character string
    const randomTag = () => {
      if (TAGS) {
        return TAGS[Math.floor(Math.random() * TAGS.length)]
      }
      // Fallback: random 3-10 character string
      const len = Math.floor(Math.random() * 8) + 3
      let result = ''
      for (let i = 0; i < len; i++) {
        result += CHARACTERS[Math.floor(Math.random() * CHARACTERS.length)]
      }
      return result
    }

    // Helper to generate tail characters
    const generateTail = (len: number) => {
      const tail: string[] = []
      for (let j = 0; j < len; j++) {
        tail.push(CHARACTERS[Math.floor(Math.random() * CHARACTERS.length)])
      }
      return tail
    }

    // Map tag frequency to speed (higher frequency = faster/bigger)
    const frequencyToSpeed = (tag: string) => {
      if (!tagData) {
        // No tag data — return random speed
        return minSpeed + Math.random() * (maxSpeed - minSpeed)
      }
      const freq = tagData[tag] || 1
      // Use log scale to compress the range
      const logFreq = Math.log(freq)
      const logMax = Math.log(MAX_FREQUENCY)
      const logMin = Math.log(MIN_FREQUENCY)
      const ratio = logMax !== logMin ? (logFreq - logMin) / (logMax - logMin) : 0.5
      return minSpeed + ratio * (maxSpeed - minSpeed)
    }

    // Create streams - horizontal flow (left to right)
    const streams: Stream[] = []
    const streamCount = Math.ceil(canvas.height / minFontSize)

    for (let i = 0; i < streamCount; i++) {
      const tailLength = Math.floor(Math.random() * 15) + 5
      const whiskerLength = Math.floor(Math.random() * 8) + 3
      const leadTag = randomTag()
      const speed = frequencyToSpeed(leadTag)
      const fontSize = speedToFontSize(speed)
      streams.push({
        y: Math.random() * canvas.height,
        x: Math.random() * canvas.width - canvas.width, // Start off-screen left
        speed,
        fontSize,
        leadTag,
        tailChars: generateTail(tailLength),
        tailLength,
        whiskerChars: generateTail(whiskerLength),
        whiskerLength,
      })
    }

    let animationId: number

    const draw = () => {
      // Clear background - true black
      ctx.fillStyle = '#000000'
      ctx.fillRect(0, 0, canvas.width, canvas.height)

      streams.forEach((stream) => {
        ctx.font = `${stream.fontSize}px monospace`
        const charWidth = stream.fontSize * 0.6 // Approximate monospace char width

        // Draw the lead tag (orange)
        ctx.fillStyle = '#ff8800'
        const tagWidth = stream.leadTag.length * charWidth
        if (stream.x > -tagWidth && stream.x < canvas.width + charWidth) {
          ctx.fillText(stream.leadTag, stream.x, stream.y)
        }

        // Draw the whisker (orange, fading) - leads in front of the tag (to the right)
        stream.whiskerChars.forEach((char, index) => {
          const charX = stream.x + tagWidth + index * charWidth

          if (charX > -charWidth && charX < canvas.width + charWidth) {
            const fadeRatio = 1 - index / stream.whiskerLength
            const alpha = Math.floor(fadeRatio * 3) / 3
            ctx.fillStyle = `rgba(255, 136, 0, ${alpha})`
            ctx.fillText(char, charX, stream.y)
          }
        })

        // Draw the tail (orange, fading) - trails behind the tag (to the left)
        stream.tailChars.forEach((char, index) => {
          const charX = stream.x - (index + 1) * charWidth

          if (charX > -charWidth && charX < canvas.width + charWidth) {
            const fadeRatio = 1 - index / stream.tailLength
            const alpha = Math.floor(fadeRatio * 3) / 3
            ctx.fillStyle = `rgba(255, 136, 0, ${alpha})`
            ctx.fillText(char, charX, stream.y)
          }
        })

        // Move stream to the right
        stream.x += stream.speed

        // Calculate tail width for reset check
        const tailWidth = stream.tailLength * charWidth

        // Reset when tail (leftmost part) is fully off screen right
        if (stream.x - tailWidth > canvas.width) {
          stream.leadTag = randomTag()
          stream.speed = frequencyToSpeed(stream.leadTag)
          stream.fontSize = speedToFontSize(stream.speed)
          stream.x = -tailWidth
          stream.y = Math.random() * canvas.height
          // Randomize tail characters
          for (let j = 0; j < stream.tailLength; j++) {
            stream.tailChars[j] = CHARACTERS[Math.floor(Math.random() * CHARACTERS.length)]
          }
          // Randomize whisker characters
          for (let j = 0; j < stream.whiskerLength; j++) {
            stream.whiskerChars[j] = CHARACTERS[Math.floor(Math.random() * CHARACTERS.length)]
          }
        }

        // Frequently change characters in tail and whisker for flicker effect
        for (let j = 0; j < stream.tailLength; j++) {
          if (Math.random() < 0.15) {
            stream.tailChars[j] = CHARACTERS[Math.floor(Math.random() * CHARACTERS.length)]
          }
        }
        for (let j = 0; j < stream.whiskerLength; j++) {
          if (Math.random() < 0.15) {
            stream.whiskerChars[j] = CHARACTERS[Math.floor(Math.random() * CHARACTERS.length)]
          }
        }
      })

      animationId = requestAnimationFrame(draw)
    }

    draw()

    return () => {
      window.removeEventListener('resize', resize)
      cancelAnimationFrame(animationId)
    }
  }, [tagData])

  return (
    <div className="loading-screen">
      <canvas ref={canvasRef} className="loading-canvas" />
      <div className="loading-overlay">
        <h1 className="loading-text">Loading Journal</h1>
      </div>
    </div>
  )
}

export default LoadingScreen
