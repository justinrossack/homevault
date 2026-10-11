import { useCallback, useEffect, useRef, useState } from 'react'
import { api } from '../api'
import type { UploadItem } from '../components/UploadPanel'

const PARALLEL_UPLOADS = 3

/**
 * Upload-Warteschlange für die ganze App: läuft weiter, wenn man den Ordner
 * oder die Ansicht wechselt. Ansichten erfahren über `onUploaded`, wenn eine
 * Datei in einem Ordner fertig ist.
 */
export function useUploads() {
  const [items, setItems] = useState<UploadItem[]>([])
  const queue = useRef<{ id: number; dir: string; file: File }[]>([])
  const active = useRef(0)
  const nextId = useRef(0)
  const listeners = useRef(new Set<(dir: string) => void>())
  const running = items.some((i) => i.status === 'waiting' || i.status === 'uploading')

  // Warnen, bevor die Seite mit laufenden Uploads verlassen wird
  useEffect(() => {
    if (!running) return
    const warn = (e: BeforeUnloadEvent) => {
      e.preventDefault()
      e.returnValue = ''
    }
    window.addEventListener('beforeunload', warn)
    return () => window.removeEventListener('beforeunload', warn)
  }, [running])

  const update = useCallback(
    (id: number, patch: Partial<UploadItem>) => setItems((u) => u.map((i) => (i.id === id ? { ...i, ...patch } : i))),
    [],
  )

  const pump = useCallback(() => {
    while (active.current < PARALLEL_UPLOADS && queue.current.length > 0) {
      const { id, dir, file } = queue.current.shift()!
      active.current++
      update(id, { status: 'uploading' })
      api
        .upload(dir, file, (progress) => update(id, { progress }))
        .then(() => {
          update(id, { status: 'done', progress: 1 })
          listeners.current.forEach((fn) => fn(dir))
        })
        .catch((e) => update(id, { status: 'error', error: (e as Error).message }))
        .finally(() => {
          active.current--
          pump()
        })
    }
  }, [update])

  const upload = useCallback(
    (dir: string, files: File[]) => {
      if (files.length === 0) return
      const added = files.map((file) => ({ id: ++nextId.current, dir, file }))
      setItems((u) => [
        ...u.filter((i) => i.status !== 'done'),
        ...added.map((a): UploadItem => ({ id: a.id, name: a.file.name, progress: 0, status: 'waiting' })),
      ])
      queue.current.push(...added)
      pump()
    },
    [pump],
  )

  const onUploaded = useCallback((fn: (dir: string) => void) => {
    listeners.current.add(fn)
    return () => {
      listeners.current.delete(fn)
    }
  }, [])

  const clear = useCallback(() => setItems([]), [])

  return { items, upload, onUploaded, clear }
}
