import { useEffect, useMemo, useState } from 'react'
import {
  Alert,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  IconButton,
  ListItemIcon,
  ListItemText,
  Menu,
  MenuItem,
  Stack,
  TextField,
  Typography,
} from '@mui/material'
import ArrowBackRoundedIcon from '@mui/icons-material/ArrowBackRounded'
import MoreVertIcon from '@mui/icons-material/MoreVert'
import DriveFileRenameOutlineRoundedIcon from '@mui/icons-material/DriveFileRenameOutlineRounded'
import DeleteOutlineRoundedIcon from '@mui/icons-material/DeleteOutlineRounded'
import RemoveCircleOutlineRoundedIcon from '@mui/icons-material/RemoveCircleOutlineRounded'
import PhotoAlbumOutlinedIcon from '@mui/icons-material/PhotoAlbumOutlined'
import ShareRoundedIcon from '@mui/icons-material/ShareRounded'
import { api, type Entry } from '../api'
import { navigate } from '../router'
import { useShell } from '../shell'
import MediaCollection, { type ExtraAction } from '../components/MediaCollection'
import { Empty, Loading, ViewHeader } from './common'

export default function AlbumView({ id }: { id: number }) {
  const shell = useShell()
  const [name, setName] = useState('')
  const [entries, setEntries] = useState<Entry[] | null>(null)
  const [error, setError] = useState('')
  const [menu, setMenu] = useState<HTMLElement | null>(null)
  const [dialog, setDialog] = useState<'rename' | 'delete' | null>(null)
  const [newName, setNewName] = useState('')

  useEffect(() => {
    setEntries(null)
    api
      .album(id)
      .then((r) => {
        setName(r.name)
        setEntries(r.entries)
      })
      .catch((e) => setError((e as Error).message))
  }, [id])

  const remove: ExtraAction = useMemo(
    () => ({
      label: 'Aus Album entfernen',
      icon: <RemoveCircleOutlineRoundedIcon fontSize="small" />,
      run: async (items) => {
        try {
          await api.removeFromAlbum(
            id,
            items.map((e) => e.path),
          )
          const gone = new Set(items.map((e) => e.path))
          setEntries((prev) => prev?.filter((e) => !gone.has(e.path)) ?? null)
          shell.toast(items.length === 1 ? 'Aus dem Album entfernt' : `${items.length} aus dem Album entfernt`)
        } catch (e) {
          shell.toast((e as Error).message)
        }
      },
    }),
    [id, shell],
  )

  async function rename() {
    try {
      await api.renameAlbum(id, newName.trim())
      setName(newName.trim())
      setDialog(null)
    } catch (e) {
      shell.toast((e as Error).message)
    }
  }

  async function del() {
    try {
      await api.deleteAlbum(id)
      shell.toast(`Album „${name}“ gelöscht`)
      navigate({ view: 'albums' })
    } catch (e) {
      shell.toast((e as Error).message)
    }
  }

  return (
    <>
      <ViewHeader
        title={
          <Stack direction="row" sx={{ alignItems: 'center', gap: 1, minWidth: 0 }}>
            <IconButton onClick={() => navigate({ view: 'albums' })} aria-label="Zurück zu den Alben" edge="start">
              <ArrowBackRoundedIcon />
            </IconButton>
            <Typography variant="h5" noWrap sx={{ fontWeight: 700 }}>
              {name}
            </Typography>
          </Stack>
        }
        subtitle={entries ? `${entries.length} ${entries.length === 1 ? 'Element' : 'Elemente'}` : undefined}
        actions={
          <Stack direction="row" sx={{ gap: 1 }}>
            <Button variant="outlined" startIcon={<ShareRoundedIcon />} onClick={() => shell.share({ album_id: id, title: name })}>
              Teilen
            </Button>
            <IconButton onClick={(e) => setMenu(e.currentTarget)} aria-label="Album-Aktionen">
              <MoreVertIcon />
            </IconButton>
          </Stack>
        }
      />
      {error && <Alert severity="error">{error}</Alert>}
      {entries === null ? (
        !error && <Loading />
      ) : entries.length === 0 ? (
        <Empty
          icon={<PhotoAlbumOutlinedIcon />}
          action={
            <Button variant="outlined" onClick={() => navigate({ view: 'timeline' })}>
              Zur Zeitleiste
            </Button>
          }
        >
          Dieses Album ist leer. Wähle in der Zeitleiste oder in einem Ordner Fotos aus und füge sie hinzu.
        </Empty>
      ) : (
        <MediaCollection entries={entries} setEntries={(fn) => setEntries((p) => fn(p ?? []))} extraAction={remove} />
      )}

      <Menu anchorEl={menu} open={!!menu} onClose={() => setMenu(null)}>
        <MenuItem
          onClick={() => {
            setNewName(name)
            setDialog('rename')
            setMenu(null)
          }}
        >
          <ListItemIcon>
            <DriveFileRenameOutlineRoundedIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>Umbenennen</ListItemText>
        </MenuItem>
        <MenuItem
          onClick={() => {
            setDialog('delete')
            setMenu(null)
          }}
          sx={{ color: 'error.main' }}
        >
          <ListItemIcon>
            <DeleteOutlineRoundedIcon fontSize="small" color="error" />
          </ListItemIcon>
          <ListItemText>Album löschen</ListItemText>
        </MenuItem>
      </Menu>

      <Dialog open={dialog === 'rename'} onClose={() => setDialog(null)} fullWidth maxWidth="xs">
        <form
          onSubmit={(e) => {
            e.preventDefault()
            if (newName.trim()) rename()
          }}
        >
          <DialogTitle>Album umbenennen</DialogTitle>
          <DialogContent>
            <TextField autoFocus fullWidth margin="dense" label="Name" value={newName} onChange={(e) => setNewName(e.target.value)} />
          </DialogContent>
          <DialogActions>
            <Button onClick={() => setDialog(null)}>Abbrechen</Button>
            <Button type="submit" variant="contained" disabled={!newName.trim()}>
              Speichern
            </Button>
          </DialogActions>
        </form>
      </Dialog>

      <Dialog open={dialog === 'delete'} onClose={() => setDialog(null)}>
        <DialogTitle>Album „{name}“ löschen?</DialogTitle>
        <DialogContent>
          <DialogContentText>Nur das Album wird gelöscht. Die Fotos und Videos bleiben in ihren Ordnern erhalten.</DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setDialog(null)}>Abbrechen</Button>
          <Button color="error" variant="contained" onClick={del}>
            Löschen
          </Button>
        </DialogActions>
      </Dialog>
    </>
  )
}
