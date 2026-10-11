import { useEffect, useState } from 'react'
import { Alert, Box, Button, ButtonBase, Dialog, DialogActions, DialogContent, DialogTitle, TextField, Typography } from '@mui/material'
import AddRoundedIcon from '@mui/icons-material/AddRounded'
import PhotoAlbumOutlinedIcon from '@mui/icons-material/PhotoAlbumOutlined'
import { api, type Album } from '../api'
import { navigate } from '../router'
import { useShell } from '../shell'
import { Empty, Loading, ViewHeader } from './common'

export default function AlbumsView() {
  const shell = useShell()
  const [albums, setAlbums] = useState<Album[] | null>(null)
  const [error, setError] = useState('')
  const [creating, setCreating] = useState(false)
  const [name, setName] = useState('')

  useEffect(() => {
    api
      .albums()
      .then((r) => setAlbums(r.albums))
      .catch((e) => setError((e as Error).message))
  }, [])

  async function create() {
    try {
      const { id } = await api.createAlbum(name.trim())
      navigate({ view: 'album', id })
    } catch (e) {
      shell.toast((e as Error).message)
    }
  }

  return (
    <>
      <ViewHeader
        title="Alben"
        actions={
          <Button
            variant="contained"
            startIcon={<AddRoundedIcon />}
            onClick={() => {
              setName('')
              setCreating(true)
            }}
          >
            Neues Album
          </Button>
        }
      />
      {error && <Alert severity="error">{error}</Alert>}
      {albums === null ? (
        !error && <Loading />
      ) : albums.length === 0 ? (
        <Empty icon={<PhotoAlbumOutlinedIcon />}>
          Noch keine Alben. Erstelle eines oder wähle in der Zeitleiste Fotos aus und füge sie einem Album hinzu. Die Fotos werden dabei
          nicht kopiert.
        </Empty>
      ) : (
        <Box
          sx={{
            display: 'grid',
            gap: 2,
            gridTemplateColumns: { xs: 'repeat(2, 1fr)', sm: 'repeat(auto-fill, minmax(200px, 1fr))' },
          }}
        >
          {albums.map((a) => (
            <ButtonBase
              key={a.id}
              onClick={() => navigate({ view: 'album', id: a.id })}
              focusRipple
              sx={{ display: 'block', textAlign: 'left', borderRadius: 3 }}
            >
              <Box
                sx={{
                  aspectRatio: '1',
                  borderRadius: 3,
                  overflow: 'hidden',
                  bgcolor: 'action.hover',
                  display: 'grid',
                  placeItems: 'center',
                }}
              >
                {a.cover ? (
                  <Box
                    component="img"
                    src={api.thumbUrl(a.cover)}
                    alt=""
                    loading="lazy"
                    sx={{ width: '100%', height: '100%', objectFit: 'cover' }}
                  />
                ) : (
                  <PhotoAlbumOutlinedIcon sx={{ fontSize: 64, color: 'text.secondary', opacity: 0.5 }} />
                )}
              </Box>
              <Typography noWrap sx={{ fontWeight: 600, mt: 1, px: 0.5 }}>
                {a.name}
              </Typography>
              <Typography variant="body2" color="text.secondary" sx={{ px: 0.5 }}>
                {a.count} {a.count === 1 ? 'Element' : 'Elemente'}
              </Typography>
            </ButtonBase>
          ))}
        </Box>
      )}

      <Dialog open={creating} onClose={() => setCreating(false)} fullWidth maxWidth="xs">
        <form
          onSubmit={(e) => {
            e.preventDefault()
            if (name.trim()) create()
          }}
        >
          <DialogTitle>Neues Album</DialogTitle>
          <DialogContent>
            <TextField autoFocus fullWidth margin="dense" label="Name" value={name} onChange={(e) => setName(e.target.value)} />
          </DialogContent>
          <DialogActions>
            <Button onClick={() => setCreating(false)}>Abbrechen</Button>
            <Button type="submit" variant="contained" disabled={!name.trim()}>
              Erstellen
            </Button>
          </DialogActions>
        </form>
      </Dialog>
    </>
  )
}
