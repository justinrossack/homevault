import { useEffect, useState } from 'react'
import { Alert } from '@mui/material'
import FavoriteBorderRoundedIcon from '@mui/icons-material/FavoriteBorderRounded'
import { api, type Entry } from '../api'
import MediaCollection from '../components/MediaCollection'
import { Empty, Loading, ViewHeader } from './common'

export default function FavoritesView() {
  const [entries, setEntries] = useState<Entry[] | null>(null)
  const [error, setError] = useState('')

  useEffect(() => {
    api
      .favorites()
      .then((r) => setEntries(r.entries))
      .catch((e) => setError((e as Error).message))
  }, [])

  return (
    <>
      <ViewHeader
        title="Favoriten"
        subtitle={entries?.length ? `${entries.length} ${entries.length === 1 ? 'Element' : 'Elemente'}` : undefined}
      />
      {error && <Alert severity="error">{error}</Alert>}
      {entries === null ? (
        !error && <Loading />
      ) : entries.length === 0 ? (
        <Empty icon={<FavoriteBorderRoundedIcon />}>
          Noch keine Favoriten. Tippe beim Ansehen eines Fotos auf das Herz, um es hier zu sammeln.
        </Empty>
      ) : (
        <MediaCollection entries={entries} setEntries={(fn) => setEntries((p) => fn(p ?? []))} groupByMonth removeWhenUnfavorited />
      )}
    </>
  )
}
