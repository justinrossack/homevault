import { useCallback, useEffect, useRef, useState } from 'react'
import { Alert, Button, LinearProgress } from '@mui/material'
import PhotoLibraryOutlinedIcon from '@mui/icons-material/PhotoLibraryOutlined'
import { api, type Entry } from '../api'
import { navigate } from '../router'
import MediaCollection from '../components/MediaCollection'
import { Empty, Loading, ViewHeader } from './common'

/** Alle Fotos und Videos nach Aufnahmedatum, über alle Ordner hinweg. */
export default function TimelineView() {
  const [entries, setEntries] = useState<Entry[]>([])
  const [next, setNext] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [indexing, setIndexing] = useState(false)
  const [error, setError] = useState('')
  const busy = useRef(false)
  const count = useRef(0)
  count.current = entries.length

  /** Nächste Seite anhängen */
  const load = useCallback(async (cursor: string) => {
    if (busy.current) return
    busy.current = true
    try {
      const r = await api.timeline(cursor)
      setEntries((prev) => [...prev, ...r.entries])
      setNext(r.next)
      setIndexing(r.indexing)
      setError('')
    } catch (e) {
      setError((e as Error).message)
    } finally {
      busy.current = false
    }
  }, [])

  /** Von vorne laden, aber mindestens so viele Einträge wie schon angezeigt (sonst springt die Ansicht) */
  const refresh = useCallback(async () => {
    if (busy.current) return
    busy.current = true
    try {
      const want = count.current
      let all: Entry[] = []
      let cursor: string | null = null
      let stillIndexing = false
      do {
        const r = await api.timeline(cursor)
        all = all.concat(r.entries)
        cursor = r.next
        stillIndexing = r.indexing
      } while (cursor && all.length < want)
      setEntries(all)
      setNext(cursor)
      setIndexing(stillIndexing)
      setError('')
    } catch (e) {
      setError((e as Error).message)
    } finally {
      busy.current = false
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    refresh()
  }, [refresh])

  // Während der erste Index läuft, regelmäßig aktualisieren
  useEffect(() => {
    if (!indexing) return
    const t = setInterval(refresh, 3000)
    return () => clearInterval(t)
  }, [indexing, refresh])

  const loadMore = useCallback(() => {
    if (next) load(next)
  }, [next, load])

  return (
    <>
      <ViewHeader title="Zeitleiste" subtitle={entries.length > 0 ? 'Alle Fotos und Videos nach Aufnahmedatum' : undefined} />
      {indexing && <LinearProgress sx={{ mb: 2, borderRadius: 1 }} />}
      {error && <Alert severity="error">{error}</Alert>}
      {loading ? (
        <Loading />
      ) : entries.length === 0 ? (
        <Empty
          icon={<PhotoLibraryOutlinedIcon />}
          action={
            <Button variant="outlined" onClick={() => navigate({ view: 'files', path: '' })}>
              Zu den Dateien
            </Button>
          }
        >
          {indexing
            ? 'Deine Fotos werden gerade erfasst …'
            : 'Noch keine Fotos oder Videos. Lade welche in „Privat“ oder „Gemeinsam“ hoch.'}
        </Empty>
      ) : (
        <MediaCollection entries={entries} setEntries={setEntries} groupByMonth onEndReached={next ? loadMore : undefined} />
      )}
    </>
  )
}
