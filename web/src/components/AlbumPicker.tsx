import { useEffect, useState } from 'react'
import {
  Avatar,
  Box,
  Button,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  List,
  ListItemAvatar,
  ListItemButton,
  ListItemText,
  Stack,
  TextField,
} from '@mui/material'
import PhotoAlbumOutlinedIcon from '@mui/icons-material/PhotoAlbumOutlined'
import AddRoundedIcon from '@mui/icons-material/AddRounded'
import { api, type Album, type Entry } from '../api'

interface Props {
  entries: Entry[] | null
  onClose: () => void
  onDone: (msg: string) => void
}

/** Dialog: Einträge zu einem bestehenden oder neuen Album hinzufügen. */
export default function AlbumPicker({ entries, onClose, onDone }: Props) {
  const [albums, setAlbums] = useState<Album[] | null>(null)
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')

  useEffect(() => {
    if (!entries) return
    setAlbums(null)
    setName('')
    setError('')
    api
      .albums()
      .then((r) => setAlbums(r.albums))
      .catch((e) => setError((e as Error).message))
  }, [entries])

  async function add(id: number, albumName: string) {
    if (!entries) return
    setBusy(true)
    try {
      const r = await api.addToAlbum(
        id,
        entries.map((e) => e.path),
      )
      onDone(r.added === 0 ? `Schon in „${albumName}“` : `${r.added} zu „${albumName}“ hinzugefügt`)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  async function createAndAdd() {
    const n = name.trim()
    if (!n) return
    setBusy(true)
    try {
      const { id } = await api.createAlbum(n)
      await add(id, n)
    } catch (e) {
      setError((e as Error).message)
      setBusy(false)
    }
  }

  return (
    <Dialog open={!!entries} onClose={onClose} fullWidth maxWidth="xs">
      <DialogTitle>{entries?.length === 1 ? 'Zu Album hinzufügen' : `${entries?.length ?? 0} Elemente zu Album hinzufügen`}</DialogTitle>
      <DialogContent>
        <Box
          component="form"
          onSubmit={(e) => {
            e.preventDefault()
            createAndAdd()
          }}
        >
          <Stack direction="row" spacing={1} sx={{ pt: 1, pb: 1 }}>
            <TextField size="small" label="Neues Album" value={name} onChange={(e) => setName(e.target.value)} sx={{ flex: 1 }} autoFocus />
            <Button type="submit" variant="contained" startIcon={<AddRoundedIcon />} disabled={busy || !name.trim()}>
              Erstellen
            </Button>
          </Stack>
        </Box>
        {error && <Box sx={{ color: 'error.main', fontSize: 14, py: 1 }}>{error}</Box>}
        {albums === null ? (
          <Box sx={{ display: 'grid', placeItems: 'center', py: 3 }}>
            <CircularProgress size={28} />
          </Box>
        ) : (
          <List dense>
            {albums.map((a) => (
              <ListItemButton key={a.id} disabled={busy} onClick={() => add(a.id, a.name)} sx={{ borderRadius: 2 }}>
                <ListItemAvatar>
                  <Avatar variant="rounded" src={a.cover ? api.thumbUrl(a.cover) : undefined}>
                    <PhotoAlbumOutlinedIcon />
                  </Avatar>
                </ListItemAvatar>
                <ListItemText primary={a.name} secondary={`${a.count} ${a.count === 1 ? 'Element' : 'Elemente'}`} />
              </ListItemButton>
            ))}
          </List>
        )}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Abbrechen</Button>
      </DialogActions>
    </Dialog>
  )
}
