import { useEffect, useRef, useState, type ReactNode } from 'react'
import { Box } from '@mui/material'

interface Props {
  /** Ziehen beginnt (Stand der Auswahl merken) */
  onStart: () => void
  /** Laufend: Pfade aller Kacheln im Rahmen; additiv bei ⇧/⌘/Strg */
  onSelect: (paths: string[], additive: boolean) => void
  children: ReactNode
}

/** Ab so vielen Pixeln Bewegung wird aus einem Klick eine Rahmenauswahl. */
const THRESHOLD = 6
/** Abstand zum Fensterrand, ab dem automatisch gescrollt wird */
const EDGE = 60

type PageRect = { l: number; t: number; r: number; b: number }

/**
 * Rahmenauswahl mit der Maus (wie im Finder/Explorer). Kacheln, die
 * ausgewählt werden können, tragen `data-select-path`. Funktioniert nur mit
 * der Maus; auf Touch-Geräten bleibt es beim langen Drücken.
 */
export default function MarqueeArea({ onStart, onSelect, children }: Props) {
  const ref = useRef<HTMLDivElement>(null)
  const [box, setBox] = useState<{ x: number; y: number; w: number; h: number } | null>(null)
  // aktuelle Callbacks, ohne die Listener neu zu registrieren
  const cb = useRef({ onStart, onSelect })
  cb.current = { onStart, onSelect }

  useEffect(() => {
    const el = ref.current
    if (!el) return
    let start: { x: number; y: number } | null = null // Seitenkoordinaten
    let pointer = { x: 0, y: 0 } // Fensterkoordinaten
    let active = false
    let additive = false
    let tiles: { path: string; rect: PageRect }[] = []
    let scrollTimer: number | undefined

    const update = () => {
      if (!start) return
      const px = pointer.x + window.scrollX
      const py = pointer.y + window.scrollY
      const sel: PageRect = { l: Math.min(start.x, px), t: Math.min(start.y, py), r: Math.max(start.x, px), b: Math.max(start.y, py) }
      setBox({ x: sel.l - window.scrollX, y: sel.t - window.scrollY, w: sel.r - sel.l, h: sel.b - sel.t })
      const hits = tiles.filter(({ rect: r }) => r.l < sel.r && r.r > sel.l && r.t < sel.b && r.b > sel.t).map((t) => t.path)
      cb.current.onSelect(hits, additive)
    }

    const activate = () => {
      active = true
      // Positionen einmal messen (in Seitenkoordinaten, damit Scrollen nichts verschiebt)
      tiles = [...el.querySelectorAll<HTMLElement>('[data-select-path]')].map((n) => {
        const r = n.getBoundingClientRect()
        return {
          path: n.dataset.selectPath!,
          rect: { l: r.left + window.scrollX, t: r.top + window.scrollY, r: r.right + window.scrollX, b: r.bottom + window.scrollY },
        }
      })
      document.body.style.userSelect = 'none'
      window.getSelection()?.removeAllRanges()
      cb.current.onStart()
      // am Fensterrand automatisch weiterscrollen
      scrollTimer = window.setInterval(() => {
        const { y } = pointer
        const speed = y < EDGE ? -(EDGE - y) / 2 : y > window.innerHeight - EDGE ? (y - (window.innerHeight - EDGE)) / 2 : 0
        if (speed) {
          window.scrollBy(0, speed)
          update()
        }
      }, 16)
    }

    const onMove = (e: PointerEvent) => {
      pointer = { x: e.clientX, y: e.clientY }
      if (!start) return
      if (!active) {
        const dx = e.pageX - start.x
        const dy = e.pageY - start.y
        if (Math.hypot(dx, dy) < THRESHOLD) return
        activate()
      }
      update()
    }

    const finish = () => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      clearInterval(scrollTimer)
      if (active) {
        // der folgende Klick soll keine Kachel öffnen
        const swallow = (ev: MouseEvent) => {
          ev.stopPropagation()
          ev.preventDefault()
        }
        window.addEventListener('click', swallow, { capture: true, once: true })
        setTimeout(() => window.removeEventListener('click', swallow, { capture: true }), 0)
        document.body.style.userSelect = ''
      }
      start = null
      active = false
      setBox(null)
    }
    const onUp = () => finish()

    const onDown = (e: PointerEvent) => {
      if (e.pointerType !== 'mouse' || e.button !== 0) return
      const t = e.target as HTMLElement
      // nicht auf Knöpfen (Auswahlkreis, Menü, Hochladen …), Ordnern oder Eingabefeldern
      // starten; auf Foto-Kacheln und freien Flächen schon
      const control = t.closest('button, a[href], input, textarea, select, [role="button"], [aria-label]')
      if (control && !(control.matches('button') && control.closest('[data-select-path]') && !control.hasAttribute('aria-label'))) return
      start = { x: e.pageX, y: e.pageY }
      pointer = { x: e.clientX, y: e.clientY }
      additive = e.shiftKey || e.metaKey || e.ctrlKey
      window.addEventListener('pointermove', onMove)
      window.addEventListener('pointerup', onUp)
    }

    // auf dem ganzen Inhaltsbereich, damit man auch neben/unter den Kacheln anfangen kann
    const area = el.closest('main') ?? el
    area.addEventListener('pointerdown', onDown as EventListener)
    return () => {
      area.removeEventListener('pointerdown', onDown as EventListener)
      finish()
    }
  }, [])

  return (
    <Box ref={ref}>
      {children}
      {box && (
        <Box
          sx={{
            position: 'fixed',
            left: box.x,
            top: box.y,
            width: box.w,
            height: box.h,
            border: '1px solid',
            borderColor: 'primary.main',
            bgcolor: 'rgba(100, 130, 255, .15)',
            borderRadius: 0.5,
            pointerEvents: 'none',
            zIndex: (t) => t.zIndex.modal,
          }}
        />
      )}
    </Box>
  )
}
