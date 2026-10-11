import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { isMedia, type Entry } from '../api'

/** Läuft gerade eine Eingabe oder ist ein Dialog offen? Dann keine Kürzel. */
function shortcutsBlocked(e: KeyboardEvent) {
  const t = e.target
  const typing = t instanceof Element && !!t.closest('input, textarea, select, [contenteditable="true"]')
  return typing || !!document.querySelector('.MuiDialog-root, .MuiMenu-root')
}

/**
 * Auswahl von Fotos/Videos wie am Desktop gewohnt:
 * Klick auf den Kreis oder ⌘/Strg-Klick schaltet um, ⇧-Klick wählt einen
 * Bereich, Rahmen ziehen wählt alles darin, ⌘A wählt alles, Esc hebt auf.
 */
export function useSelection(entries: Entry[]) {
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const current = useRef(selected)
  current.current = selected
  const anchor = useRef<string | null>(null)
  const base = useRef<Set<string>>(new Set())
  const selectable = useMemo(() => entries.filter(isMedia), [entries])

  const toggle = useCallback(
    (e: Entry, range = false) => {
      setSelected((s) => {
        const next = new Set(s)
        if (range && anchor.current) {
          const order = selectable.map((x) => x.path)
          const a = order.indexOf(anchor.current)
          const b = order.indexOf(e.path)
          if (a >= 0 && b >= 0) {
            for (const p of order.slice(Math.min(a, b), Math.max(a, b) + 1)) next.add(p)
            return next
          }
        }
        if (next.has(e.path)) next.delete(e.path)
        else next.add(e.path)
        return next
      })
      anchor.current = e.path
    },
    [selectable],
  )

  const clear = useCallback(() => setSelected(new Set()), [])

  /** Rahmenauswahl: Stand zu Beginn merken, dann laufend aktualisieren */
  const beginMarquee = useCallback(() => {
    base.current = current.current
  }, [])
  const marquee = useCallback((hits: string[], additive: boolean) => {
    setSelected(additive ? new Set([...base.current, ...hits]) : new Set(hits))
    if (hits.length) anchor.current = hits[hits.length - 1]
  }, [])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (shortcutsBlocked(e)) return
      if (e.key === 'Escape' && current.current.size > 0) {
        setSelected(new Set())
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'a' && selectable.length > 0) {
        e.preventDefault()
        setSelected(new Set(selectable.map((x) => x.path)))
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [selectable])

  const selectedEntries = useMemo(() => entries.filter((e) => selected.has(e.path)), [entries, selected])

  return { selected, selectedEntries, toggle, clear, beginMarquee, marquee }
}
