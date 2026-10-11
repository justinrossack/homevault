import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  Alert,
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  IconButton,
  ListItemIcon,
  ListItemText,
  Menu,
  MenuItem,
  Stack,
  TextField,
  Tooltip,
  Typography,
} from '@mui/material'
import CloudUploadRoundedIcon from '@mui/icons-material/CloudUploadRounded'
import CreateNewFolderRoundedIcon from '@mui/icons-material/CreateNewFolderRounded'
import RefreshRoundedIcon from '@mui/icons-material/RefreshRounded'
import DriveFileRenameOutlineRoundedIcon from '@mui/icons-material/DriveFileRenameOutlineRounded'
import DownloadRoundedIcon from '@mui/icons-material/DownloadRounded'
import DeleteOutlineRoundedIcon from '@mui/icons-material/DeleteOutlineRounded'
import FolderOpenRoundedIcon from '@mui/icons-material/FolderOpenRounded'
import AdjustRoundedIcon from '@mui/icons-material/AdjustRounded'
import FavoriteRoundedIcon from '@mui/icons-material/FavoriteRounded'
import FavoriteBorderRoundedIcon from '@mui/icons-material/FavoriteBorderRounded'
import PhotoAlbumOutlinedIcon from '@mui/icons-material/PhotoAlbumOutlined'
import ShareRoundedIcon from '@mui/icons-material/ShareRounded'
import { api, isMedia, joinPath, nextRemaining, type Entry } from '../api'
import { navigate } from '../router'
import { useShell } from '../shell'
import FileGrid from '../components/FileGrid'
import Lightbox from '../components/Lightbox'
import PathBar from '../components/PathBar'
import SelectionBar from '../components/SelectionBar'
import MarqueeArea from '../components/MarqueeArea'
import { useSelection } from '../hooks/useSelection'
import { Empty, Loading } from './common'

type DialogState = { type: 'mkdir' } | { type: 'rename'; entry: Entry } | null

export default function FilesView({ path }: { path: string }) {
  const shell = useShell()
  const [entries, setEntries] = useState<Entry[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  // Pfad statt Index, damit die Lightbox beim Neuladen auf demselben Bild bleibt
  const [lightboxPath, setLightboxPath] = useState<string | null>(null)
  const [menu, setMenu] = useState<{ entry: Entry; anchor: HTMLElement } | null>(null)
  const [dialog, setDialog] = useState<DialogState>(null)
  const [dragging, setDragging] = useState(false)
  // In der Übersicht (Privat/Gemeinsam) kann nichts abgelegt werden
  const atRoot = path === ''
  const fileInput = useRef<HTMLInputElement>(null)
  const pathRef = useRef(path)
  pathRef.current = path

  // nur die Antwort der zuletzt gestarteten Anfrage übernehmen
  const loadSeq = useRef(0)
  const load = useCallback(async (p: string, quiet = false) => {
    const seq = ++loadSeq.current
    const latest = () => seq === loadSeq.current && pathRef.current === p
    if (!quiet) setLoading(true)
    try {
      const r = await api.list(p)
      if (latest()) {
        setEntries(r.entries)
        setError('')
      }
    } catch (e) {
      if (latest()) setError((e as Error).message)
    } finally {
      if (latest()) setLoading(false)
    }
  }, [])

  const { selected, selectedEntries, toggle, clear, beginMarquee, marquee } = useSelection(entries)

  useEffect(() => {
    clear()
    setLightboxPath(null)
    setMenu(null)
    setDialog(null)
    load(path)
  }, [path, load, clear])

  // Nach Uploads gesammelt neu laden: höchstens alle 500 ms, die letzte Datei immer
  const reloadTimer = useRef<number | undefined>(undefined)
  useEffect(() => {
    const off = shell.onUploaded((dir) => {
      if (dir !== pathRef.current || reloadTimer.current !== undefined) return
      reloadTimer.current = window.setTimeout(() => {
        reloadTimer.current = undefined
        if (pathRef.current === dir) load(dir, true)
      }, 500)
    })
    return () => {
      off()
      clearTimeout(reloadTimer.current)
      reloadTimer.current = undefined
    }
  }, [shell, load])

  const goto = (p: string) => navigate({ view: 'files', path: p })
  // alles außer Ordnern lässt sich in der Vollbildansicht öffnen (Fotos, Videos, Dokumente …)
  const viewable = useMemo(() => entries.filter((e) => e.kind !== 'dir'), [entries])
  const lightbox = lightboxPath === null ? -1 : viewable.findIndex((e) => e.path === lightboxPath)
  // Eintrag ist verschwunden: Lightbox zu, damit sie nicht später von selbst wieder aufgeht
  useEffect(() => {
    if (lightboxPath !== null && lightbox < 0) setLightboxPath(null)
  }, [lightboxPath, lightbox])

  const open = useCallback((e: Entry) => {
    if (e.kind === 'dir') goto(e.path)
    else setLightboxPath(e.path)
  }, [])
  const openMenu = useCallback((entry: Entry, anchor: HTMLElement) => setMenu({ entry, anchor }), [])

  // Gelöschtes aus der Ansicht nehmen; Lightbox bleibt beim nächsten Bild
  const removeDeleted = useCallback(
    (paths: Set<string>) => {
      setEntries((prev) => prev.filter((e) => !paths.has(e.path)))
      setLightboxPath((cur) => (cur === null || !paths.has(cur) ? cur : nextRemaining(viewable, cur, paths)))
    },
    [viewable],
  )
  const askDelete = useCallback((items: Entry[]) => shell.deleteEntries(items, removeDeleted), [shell, removeDeleted])

  const applyFavorite = useCallback((changed: Entry[], favorite: boolean) => {
    const paths = new Set(changed.map((e) => e.path))
    setEntries((prev) => prev.map((e) => (paths.has(e.path) ? { ...e, favorite } : e)))
  }, [])
  const toggleFavorite = useCallback(
    async (e: Entry) => {
      if (await shell.setFavorite([e], !e.favorite)) applyFavorite([e], !e.favorite)
    },
    [shell, applyFavorite],
  )

  // ---------- Upload ----------
  function startUpload(files: FileList | File[]) {
    if (atRoot) {
      shell.toast('Bitte zuerst „Privat“ oder „Gemeinsam“ öffnen')
      return
    }
    shell.upload(path, Array.from(files))
  }

  // Drag & Drop auf die ganze Ansicht
  const dragDepth = useRef(0)
  const dropHandlers = {
    onDragEnter: (e: React.DragEvent) => {
      if (!e.dataTransfer.types.includes('Files')) return
      dragDepth.current++
      setDragging(true)
    },
    onDragLeave: () => {
      if (--dragDepth.current <= 0) setDragging(false)
    },
    onDragOver: (e: React.DragEvent) => e.preventDefault(),
    onDrop: (e: React.DragEvent) => {
      e.preventDefault()
      dragDepth.current = 0
      setDragging(false)
      startUpload(e.dataTransfer.files)
    },
  }

  async function run(action: () => Promise<unknown>, success: string) {
    try {
      await action()
      shell.toast(success)
      setDialog(null)
      load(path, true)
    } catch (e) {
      shell.toast((e as Error).message)
    }
  }

  const menuItem = (icon: React.ReactNode, label: string, onClick: () => void, extra: object = {}) => (
    <MenuItem
      {...extra}
      onClick={() => {
        onClick()
        setMenu(null)
      }}
    >
      <ListItemIcon>{icon}</ListItemIcon>
      <ListItemText>{label}</ListItemText>
    </MenuItem>
  )

  return (
    <Box {...dropHandlers} sx={{ minHeight: '70dvh' }}>
      <SelectionBar selected={selectedEntries} onClear={clear} onFavorite={applyFavorite} onDeleted={removeDeleted} />

      <Stack direction="row" sx={{ alignItems: 'center', mb: 2, minHeight: 44, gap: 0.5 }}>
        <Box sx={{ flex: 1, minWidth: 0 }}>
          <PathBar path={path} onNavigate={goto} />
        </Box>
        {!loading && (
          <Typography variant="body2" color="text.secondary" sx={{ whiteSpace: 'nowrap', mx: 1, display: { xs: 'none', sm: 'block' } }}>
            {entries.length} {entries.length === 1 ? 'Eintrag' : 'Einträge'}
          </Typography>
        )}
        <Tooltip title="Neuer Ordner">
          <span>
            <IconButton disabled={atRoot} onClick={() => setDialog({ type: 'mkdir' })}>
              <CreateNewFolderRoundedIcon />
            </IconButton>
          </span>
        </Tooltip>
        <Tooltip title="Aktualisieren">
          <IconButton onClick={() => load(path)}>
            <RefreshRoundedIcon />
          </IconButton>
        </Tooltip>
        <Button
          variant="contained"
          startIcon={<CloudUploadRoundedIcon />}
          disabled={atRoot}
          onClick={() => fileInput.current?.click()}
          sx={{ ml: 0.5, '& .MuiButton-startIcon': { mr: { xs: 0, sm: 1 } } }}
        >
          <Box component="span" sx={{ display: { xs: 'none', sm: 'inline' } }}>
            Hochladen
          </Box>
        </Button>
        <input
          ref={fileInput}
          type="file"
          multiple
          hidden
          onChange={(e) => {
            if (e.target.files) startUpload(e.target.files)
            e.target.value = ''
          }}
        />
      </Stack>

      {error ? (
        <Alert severity="error" action={<Button onClick={() => goto('')}>Zur Übersicht</Button>}>
          {error}
        </Alert>
      ) : loading ? (
        <Loading />
      ) : entries.length === 0 ? (
        <Empty
          icon={<FolderOpenRoundedIcon />}
          action={
            <Button variant="outlined" startIcon={<CloudUploadRoundedIcon />} onClick={() => fileInput.current?.click()}>
              Dateien auswählen
            </Button>
          }
        >
          Dieser Ordner ist leer. Zieh Fotos oder Videos hier hinein.
        </Empty>
      ) : (
        <MarqueeArea onStart={beginMarquee} onSelect={marquee}>
          <FileGrid entries={entries} onOpen={open} onMenu={openMenu} selected={selected} onToggleSelect={toggle} />
        </MarqueeArea>
      )}

      {dragging && (
        <Box
          sx={{
            position: 'fixed',
            inset: 0,
            zIndex: (t) => t.zIndex.modal,
            pointerEvents: 'none',
            display: 'grid',
            placeItems: 'center',
            bgcolor: 'rgba(63,91,216,.15)',
            border: '3px dashed',
            borderColor: 'primary.main',
            backdropFilter: 'blur(2px)',
          }}
        >
          <Stack sx={{ alignItems: 'center', gap: 1, color: 'primary.main' }}>
            <CloudUploadRoundedIcon sx={{ fontSize: 80 }} />
            <Typography variant="h6">{atRoot ? 'Bitte zuerst einen Bereich öffnen' : 'Loslassen zum Hochladen'}</Typography>
          </Stack>
        </Box>
      )}

      <Menu anchorEl={menu?.anchor} open={!!menu} onClose={() => setMenu(null)}>
        {menu &&
          isMedia(menu.entry) && [
            menuItem(
              menu.entry.favorite ? <FavoriteRoundedIcon fontSize="small" /> : <FavoriteBorderRoundedIcon fontSize="small" />,
              menu.entry.favorite ? 'Aus Favoriten entfernen' : 'Zu Favoriten',
              () => toggleFavorite(menu.entry),
              { key: 'fav' },
            ),
            menuItem(<PhotoAlbumOutlinedIcon fontSize="small" />, 'Zu Album hinzufügen', () => shell.addToAlbum([menu.entry]), {
              key: 'album',
            }),
            menuItem(
              <ShareRoundedIcon fontSize="small" />,
              'Teilen',
              () => shell.share({ path: menu.entry.path, title: menu.entry.name }),
              {
                key: 'share',
              },
            ),
          ]}
        {menu &&
          menuItem(<DriveFileRenameOutlineRoundedIcon fontSize="small" />, 'Umbenennen', () =>
            setDialog({ type: 'rename', entry: menu.entry }),
          )}
        {menu &&
          menu.entry.kind !== 'dir' &&
          menuItem(<DownloadRoundedIcon fontSize="small" />, 'Herunterladen', () => {}, {
            component: 'a',
            href: api.downloadUrl(menu.entry.path),
          })}
        {menu?.entry.live &&
          menuItem(<AdjustRoundedIcon fontSize="small" />, 'Live-Video herunterladen', () => {}, {
            component: 'a',
            href: api.downloadUrl(menu.entry.live),
          })}
        {menu &&
          menuItem(<DeleteOutlineRoundedIcon fontSize="small" color="error" />, 'Löschen', () => askDelete([menu.entry]), {
            sx: { color: 'error.main' },
          })}
      </Menu>

      <NameDialog
        open={dialog?.type === 'mkdir'}
        title="Neuer Ordner"
        label="Ordnername"
        initial=""
        confirm="Erstellen"
        onClose={() => setDialog(null)}
        onSubmit={(name) => run(() => api.mkdir(joinPath(path, name)), `Ordner „${name}“ erstellt`)}
      />
      <NameDialog
        open={dialog?.type === 'rename'}
        title="Umbenennen"
        label="Neuer Name"
        initial={dialog?.type === 'rename' ? dialog.entry.name : ''}
        confirm="Umbenennen"
        onClose={() => setDialog(null)}
        onSubmit={(name) => dialog?.type === 'rename' && run(() => api.rename(dialog.entry.path, joinPath(path, name)), 'Umbenannt')}
      />
      <Lightbox
        items={viewable}
        index={lightbox < 0 ? null : lightbox}
        onIndex={(i) => setLightboxPath(i === null ? null : (viewable[i]?.path ?? null))}
        onToggleFavorite={toggleFavorite}
        onAddToAlbum={(e) => shell.addToAlbum([e])}
        onShare={(e) => shell.share({ path: e.path, title: e.name })}
        onDelete={(e) => askDelete([e])}
      />
    </Box>
  )
}

function NameDialog(props: {
  open: boolean
  title: string
  label: string
  initial: string
  confirm: string
  onClose: () => void
  onSubmit: (name: string) => void
}) {
  const [name, setName] = useState(props.initial)
  useEffect(() => {
    if (props.open) setName(props.initial)
  }, [props.open, props.initial])

  return (
    <Dialog open={props.open} onClose={props.onClose} fullWidth maxWidth="xs">
      <form
        onSubmit={(e) => {
          e.preventDefault()
          if (name.trim()) props.onSubmit(name.trim())
        }}
      >
        <DialogTitle>{props.title}</DialogTitle>
        <DialogContent>
          <TextField
            autoFocus
            fullWidth
            margin="dense"
            label={props.label}
            value={name}
            onChange={(e) => setName(e.target.value)}
            onFocus={(e) => {
              // nur den Namen ohne Endung markieren
              const dot = e.target.value.lastIndexOf('.')
              e.target.setSelectionRange(0, dot > 0 ? dot : e.target.value.length)
            }}
          />
        </DialogContent>
        <DialogActions>
          <Button onClick={props.onClose}>Abbrechen</Button>
          <Button type="submit" variant="contained" disabled={!name.trim()}>
            {props.confirm}
          </Button>
        </DialogActions>
      </form>
    </Dialog>
  )
}
