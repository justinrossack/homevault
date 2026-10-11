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

  const load = useCallback(async (cursor: string | null) => {
    if (busy.current) return
    busy.current = true
    try {
      const r = await api.timeline(cursor)
      setEntries((prev) => (cursor ? [...prev, ...r.entries] : r.entries))
      setNext(r.next)
      setIndexing(r.indexing)
      setError('')
    } catch (e) {
      setError((e as Error).message)
    } finally {
      busy.current = false
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    load(null)
  }, [load])

  // Während der erste Index läuft, regelmäßig neu laden
  useEffect(() => {
    if (!indexing) return
    const t = setInterval(() => load(null), 3000)
    return () => clearInterval(t)
  }, [indexing, load])

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
