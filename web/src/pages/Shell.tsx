import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react'
import {
  Alert,
  AppBar,
  Avatar,
  BottomNavigation,
  BottomNavigationAction,
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  Divider,
  Drawer,
  IconButton,
  List,
  ListItemButton,
  ListItemIcon,
  ListItemText,
  LinearProgress,
  Menu,
  MenuItem,
  Paper,
  Snackbar,
  Toolbar,
  Tooltip,
  Typography,
  useColorScheme,
} from '@mui/material'
import FolderRoundedIcon from '@mui/icons-material/FolderRounded'
import PhotoLibraryRoundedIcon from '@mui/icons-material/PhotoLibraryRounded'
import FavoriteRoundedIcon from '@mui/icons-material/FavoriteRounded'
import PhotoAlbumRoundedIcon from '@mui/icons-material/PhotoAlbumRounded'
import SettingsRoundedIcon from '@mui/icons-material/SettingsRounded'
import DarkModeRoundedIcon from '@mui/icons-material/DarkModeRounded'
import LightModeRoundedIcon from '@mui/icons-material/LightModeRounded'
import LogoutRoundedIcon from '@mui/icons-material/LogoutRounded'
import KeyRoundedIcon from '@mui/icons-material/KeyRounded'
import DeleteRoundedIcon from '@mui/icons-material/DeleteRounded'
import WarningAmberRoundedIcon from '@mui/icons-material/WarningAmberRounded'
import { api, formatSize, type Entry, type Storage, type User } from '../api'
import { hrefFor, navigate, useRoute, type Route } from '../router'
import { ShellContext, type ShellApi } from '../shell'
import AlbumPicker from '../components/AlbumPicker'
import PasswordDialog from '../components/PasswordDialog'
import ShareDialog, { type ShareTarget } from '../components/ShareDialog'
import UploadPanel from '../components/UploadPanel'
import { useUploads } from '../hooks/useUploads'
import TrashView from '../views/TrashView'
import FilesView from '../views/FilesView'
import TimelineView from '../views/TimelineView'
import FavoritesView from '../views/FavoritesView'
import AlbumsView from '../views/AlbumsView'
import AlbumView from '../views/AlbumView'
import SettingsView from '../views/SettingsView'

const DRAWER = 232

type Section = 'files' | 'timeline' | 'favorites' | 'albums' | 'trash' | 'settings'

const NAV: { section: Section; label: string; icon: ReactNode; route: Route }[] = [
  { section: 'files', label: 'Dateien', icon: <FolderRoundedIcon />, route: { view: 'files', path: '' } },
  { section: 'timeline', label: 'Zeitleiste', icon: <PhotoLibraryRoundedIcon />, route: { view: 'timeline' } },
  { section: 'favorites', label: 'Favoriten', icon: <FavoriteRoundedIcon />, route: { view: 'favorites' } },
  { section: 'albums', label: 'Alben', icon: <PhotoAlbumRoundedIcon />, route: { view: 'albums' } },
]

function sectionOf(route: Route): Section {
  return route.view === 'album' ? 'albums' : route.view
}

interface Props {
  user: User
  serverName: string
  onLogout: () => void
  refreshMe: () => void
}

export default function Shell({ user, serverName, onLogout, refreshMe }: Props) {
  const route = useRoute()
  const section = sectionOf(route)
  const [toast, setToast] = useState('')
  const [albumFor, setAlbumFor] = useState<Entry[] | null>(null)
  const [accountMenu, setAccountMenu] = useState<HTMLElement | null>(null)
  const [passwordOpen, setPasswordOpen] = useState(false)
  const [toDelete, setToDelete] = useState<{ entries: Entry[]; onDeleted: (paths: Set<string>) => void } | null>(null)
  const [deleting, setDeleting] = useState(false)
  const [shareTarget, setShareTarget] = useState<ShareTarget | null>(null)
  const [storage, setStorage] = useState<Storage | null>(null)
  const [diskState, setDiskState] = useState<string>('ok')
  const uploads = useUploads()
  const { upload, onUploaded } = uploads

  // Admins: Warnung, wenn eine Platte fehlt oder Fehler meldet
  useEffect(() => {
    if (!user.admin) return
    const load = () =>
      api
        .disksSummary()
        .then((r) => setDiskState(r.state))
        .catch(() => {})
    load()
    const t = setInterval(load, 30000)
    return () => clearInterval(t)
  }, [user.admin])

  // Speicheranzeige in der Seitenleiste
  useEffect(() => {
    const load = () =>
      api
        .storage()
        .then(setStorage)
        .catch(() => {})
    load()
    const t = setInterval(load, 60000)
    return () => clearInterval(t)
  }, [])

  async function confirmDelete() {
    if (!toDelete) return
    setDeleting(true)
    const done = new Set<string>()
    let error = ''
    for (const e of toDelete.entries) {
      try {
        await api.remove(e.path)
        done.add(e.path)
      } catch (err) {
        error = `${e.name}: ${(err as Error).message}`
      }
    }
    toDelete.onDeleted(done)
    setToast(error || (done.size === 1 ? 'In den Papierkorb verschoben' : `${done.size} in den Papierkorb verschoben`))
    setDeleting(false)
    setToDelete(null)
  }

  const setFavorite = useCallback(async (entries: Entry[], favorite: boolean) => {
    try {
      await api.setFavorite(
        entries.map((e) => e.path),
        favorite,
      )
      if (entries.length > 1) setToast(favorite ? `${entries.length} zu Favoriten hinzugefügt` : `${entries.length} aus Favoriten entfernt`)
      return true
    } catch (e) {
      setToast((e as Error).message)
      return false
    }
  }, [])

  const shell: ShellApi = useMemo(
    () => ({
      user,
      serverName,
      toast: setToast,
      setFavorite,
      addToAlbum: setAlbumFor,
      deleteEntries: (entries: Entry[], onDeleted: (paths: Set<string>) => void) => setToDelete({ entries, onDeleted }),
      share: setShareTarget,
      refreshMe,
      upload,
      onUploaded,
    }),
    [user, serverName, setFavorite, refreshMe, upload, onUploaded],
  )

  const nav = [
    ...NAV,
    { section: 'trash' as const, label: 'Papierkorb', icon: <DeleteRoundedIcon />, route: { view: 'trash' } as Route },
    { section: 'settings' as const, label: 'Einstellungen', icon: <SettingsRoundedIcon />, route: { view: 'settings' } as Route },
  ]

  let view: ReactNode
  switch (route.view) {
    case 'files':
      view = <FilesView path={route.path} />
      break
    case 'timeline':
      view = <TimelineView />
      break
    case 'favorites':
      view = <FavoritesView />
      break
    case 'albums':
      view = <AlbumsView />
      break
    case 'album':
      view = <AlbumView key={route.id} id={route.id} />
      break
    case 'settings':
      view = <SettingsView tab={route.tab} />
      break
    case 'trash':
      view = <TrashView />
      break
  }

  return (
    <ShellContext.Provider value={shell}>
      <Box sx={{ display: 'flex', minHeight: '100dvh', bgcolor: 'background.default' }}>
        <AppBar
          position="fixed"
          color="inherit"
          elevation={0}
          sx={{ borderBottom: 1, borderColor: 'divider', bgcolor: 'background.paper', zIndex: (t) => t.zIndex.drawer + 1 }}
        >
          <Toolbar sx={{ gap: 1 }}>
            <Box
              component="a"
              href={hrefFor({ view: 'files', path: '' })}
              sx={{ display: 'flex', alignItems: 'center', gap: 1, color: 'inherit', textDecoration: 'none', minWidth: 0 }}
            >
              <PhotoLibraryRoundedIcon color="primary" />
              <Typography variant="h6" noWrap sx={{ fontWeight: 700 }}>
                {serverName}
              </Typography>
            </Box>
            <Box sx={{ flex: 1 }} />
            <ThemeToggle />
            <Tooltip title={user.name}>
              <IconButton onClick={(e) => setAccountMenu(e.currentTarget)} sx={{ p: 0.5 }}>
                <Avatar sx={{ width: 32, height: 32, fontSize: 15, bgcolor: 'primary.main', color: 'primary.contrastText' }}>
                  {user.name[0].toUpperCase()}
                </Avatar>
              </IconButton>
            </Tooltip>
          </Toolbar>
        </AppBar>

        {/* Desktop: feste Seitenleiste */}
        <Drawer
          variant="permanent"
          sx={{
            display: { xs: 'none', md: 'block' },
            width: DRAWER,
            flexShrink: 0,
            '& .MuiDrawer-paper': { width: DRAWER, boxSizing: 'border-box', borderRight: 0, bgcolor: 'background.default' },
          }}
        >
          <Toolbar />
          <List sx={{ px: 1.5, pt: 1.5 }}>
            {nav.map((n) => (
              <ListItemButton
                key={n.section}
                selected={section === n.section}
                onClick={() => navigate(n.route)}
                sx={{ borderRadius: 6, mb: 0.5, '&.Mui-selected': { bgcolor: 'action.selected' } }}
              >
                <ListItemIcon sx={{ minWidth: 40, color: section === n.section ? 'primary.main' : undefined }}>{n.icon}</ListItemIcon>
                <ListItemText primary={n.label} slotProps={{ primary: { sx: { fontWeight: section === n.section ? 700 : 500 } } }} />
              </ListItemButton>
            ))}
          </List>
          <Box sx={{ flex: 1 }} />
          {storage && <StorageMeter storage={storage} />}
        </Drawer>

        <Box component="main" sx={{ flex: 1, minWidth: 0, px: { xs: 1.5, sm: 3 }, pb: { xs: 10, md: 4 } }}>
          <Toolbar />
          {(diskState === 'degraded' || diskState === 'warning' || diskState === 'unreachable') && (
            <Alert
              severity={diskState === 'warning' ? 'warning' : 'error'}
              sx={{ mt: 2 }}
              action={
                <Button color="inherit" size="small" href={hrefFor({ view: 'settings', tab: 'festplatten' })}>
                  Ansehen
                </Button>
              }
            >
              {diskState === 'degraded'
                ? 'Festplatten im Notbetrieb: Eine Platte fehlt oder ist defekt. Deine Daten sind nicht mehr doppelt gesichert.'
                : diskState === 'warning'
                  ? 'Eine Festplatte meldet Fehler und sollte bald getauscht werden.'
                  : 'Der Platten-Dienst antwortet nicht.'}
            </Alert>
          )}
          <Box sx={{ pt: 2 }}>{view}</Box>
        </Box>

        {/* Handy: untere Navigation */}
        <Paper
          elevation={8}
          sx={{ display: { xs: 'block', md: 'none' }, position: 'fixed', left: 0, right: 0, bottom: 0, zIndex: (t) => t.zIndex.appBar }}
        >
          <BottomNavigation value={section} onChange={(_, v: Section) => navigate(nav.find((n) => n.section === v)!.route)}>
            {NAV.map((n) => (
              <BottomNavigationAction key={n.section} value={n.section} label={n.label} icon={n.icon} />
            ))}
          </BottomNavigation>
        </Paper>
      </Box>

      <Menu anchorEl={accountMenu} open={!!accountMenu} onClose={() => setAccountMenu(null)}>
        <Box sx={{ px: 2, py: 1 }}>
          <Typography sx={{ fontWeight: 600 }}>{user.name}</Typography>
          <Typography variant="body2" color="text.secondary">
            {user.admin ? 'Administrator' : 'Benutzer'}
          </Typography>
        </Box>
        <Divider />
        <MenuItem
          onClick={() => {
            setPasswordOpen(true)
            setAccountMenu(null)
          }}
        >
          <ListItemIcon>
            <KeyRoundedIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>Passwort ändern</ListItemText>
        </MenuItem>
        {/* auf dem Handy nicht in der unteren Leiste */}
        <MenuItem
          onClick={() => {
            navigate({ view: 'trash' })
            setAccountMenu(null)
          }}
        >
          <ListItemIcon>
            <DeleteRoundedIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>Papierkorb</ListItemText>
        </MenuItem>
        <MenuItem
          onClick={() => {
            navigate({ view: 'settings' })
            setAccountMenu(null)
          }}
        >
          <ListItemIcon>
            <SettingsRoundedIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>Einstellungen</ListItemText>
        </MenuItem>
        <MenuItem onClick={() => api.logout().finally(onLogout)}>
          <ListItemIcon>
            <LogoutRoundedIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>Abmelden</ListItemText>
        </MenuItem>
      </Menu>

      <PasswordDialog
        open={passwordOpen}
        onClose={() => setPasswordOpen(false)}
        onDone={() => {
          setPasswordOpen(false)
          setToast('Passwort geändert')
        }}
      />
      <Dialog open={!!toDelete} onClose={() => !deleting && setToDelete(null)}>
        <DialogTitle>
          {toDelete?.entries.length === 1 ? `„${toDelete.entries[0].name}“ löschen?` : `${toDelete?.entries.length} Elemente löschen?`}
        </DialogTitle>
        <DialogContent>
          <DialogContentText>
            {toDelete?.entries.some((e) => e.kind === 'dir') ? 'Ordner werden samt Inhalt' : 'Die Auswahl wird'} in den Papierkorb
            verschoben und kann dort wiederhergestellt werden.
            {toDelete?.entries.some((e) => e.live) && ' Zugehörige Live-Videos werden mit verschoben.'}
          </DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setToDelete(null)} disabled={deleting}>
            Abbrechen
          </Button>
          <Button color="error" variant="contained" onClick={confirmDelete} disabled={deleting}>
            Löschen
          </Button>
        </DialogActions>
      </Dialog>
      <ShareDialog target={shareTarget} onClose={() => setShareTarget(null)} onToast={setToast} />
      <AlbumPicker
        entries={albumFor}
        onClose={() => setAlbumFor(null)}
        onDone={(msg) => {
          setAlbumFor(null)
          setToast(msg)
        }}
      />
      <UploadPanel items={uploads.items} onClose={uploads.clear} />
      <Snackbar
        open={!!toast}
        autoHideDuration={3000}
        onClose={() => setToast('')}
        message={toast}
        anchorOrigin={{ vertical: 'bottom', horizontal: 'left' }}
        sx={{ bottom: { xs: 72, md: 24 } }}
      />
    </ShellContext.Provider>
  )
}

function ThemeToggle() {
  const { mode, systemMode, setMode } = useColorScheme()
  const current = mode === 'system' ? systemMode : mode
  if (!current) return null
  return (
    <Tooltip title={current === 'dark' ? 'Heller Modus' : 'Dunkler Modus'}>
      <IconButton onClick={() => setMode(current === 'dark' ? 'light' : 'dark')}>
        {current === 'dark' ? <LightModeRoundedIcon /> : <DarkModeRoundedIcon />}
      </IconButton>
    </Tooltip>
  )
}

/** „X von Y frei“ unten in der Seitenleiste, mit eigenem Limit, falls gesetzt. */
function StorageMeter({ storage }: { storage: Storage }) {
  const { disk, me, warning } = storage
  const limited = me.quota != null
  const used = limited ? me.used : disk.total - disk.free
  const total = limited ? me.quota! : disk.total
  if (!total) return null
  const pct = Math.min(100, (used / total) * 100)
  const critical = warning || pct > 90
  return (
    <Box
      component="a"
      href={hrefFor({ view: 'settings', tab: 'speicher' })}
      sx={{ display: 'block', px: 3, py: 2, color: 'text.secondary', textDecoration: 'none', fontSize: 13 }}
    >
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.75, mb: 0.75, color: critical ? 'error.main' : undefined }}>
        {critical && <WarningAmberRoundedIcon sx={{ fontSize: 16 }} />}
        {limited ? 'Dein Speicher' : 'Speicher'}
      </Box>
      <LinearProgress
        variant="determinate"
        value={pct}
        color={critical ? 'error' : 'primary'}
        sx={{ borderRadius: 1, height: 6, mb: 0.75 }}
      />
      {formatSize(used)} von {formatSize(total)} belegt
    </Box>
  )
}
