import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  Alert,
  Avatar,
  Box,
  Button,
  ButtonBase,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  IconButton,
  InputAdornment,
  LinearProgress,
  ListItemIcon,
  ListItemText,
  Menu,
  MenuItem,
  Stack,
  TextField,
  Tooltip,
  Typography,
} from '@mui/material'
import PersonAddRoundedIcon from '@mui/icons-material/PersonAddRounded'
import KeyRoundedIcon from '@mui/icons-material/KeyRounded'
import DeleteOutlineRoundedIcon from '@mui/icons-material/DeleteOutlineRounded'
import AdminPanelSettingsRoundedIcon from '@mui/icons-material/AdminPanelSettingsRounded'
import PersonRoundedIcon from '@mui/icons-material/PersonRounded'
import MoreVertIcon from '@mui/icons-material/MoreVert'
import CasinoRoundedIcon from '@mui/icons-material/CasinoRounded'
import VisibilityRoundedIcon from '@mui/icons-material/VisibilityRounded'
import VisibilityOffRoundedIcon from '@mui/icons-material/VisibilityOffRounded'
import ContentCopyRoundedIcon from '@mui/icons-material/ContentCopyRounded'
import StorageRoundedIcon from '@mui/icons-material/StorageRounded'
import CheckCircleRoundedIcon from '@mui/icons-material/CheckCircleRounded'
import { api, formatSize, type Storage, type User } from '../api'
import { copyText } from './ShareDialog'

const GB = 1024 ** 3

/** GB-Eingabe lesen: leer = unbegrenzt (0), null = keine gültige Zahl */
function parseGb(text: string): number | null {
  const t = text.trim().replace(',', '.')
  if (!t) return 0
  const v = Number(t)
  return Number.isFinite(v) && v >= 0 ? v : null
}
const GB_ERROR = 'Bitte eine Zahl eingeben, z. B. 50 oder 2,5'

interface Props {
  me: User
  onToast: (msg: string) => void
}

type UserRow = User & { created_at: number; quota_bytes?: number | null }

/** Gut lesbares Zufallspasswort (ohne verwechselbare Zeichen wie l/1, O/0). */
export function generatePassword(length = 14) {
  const chars = 'abcdefghjkmnpqrstuvwxyzABCDEFGHJKMNPQRSTUVWXYZ23456789'
  const bytes = crypto.getRandomValues(new Uint32Array(length))
  return Array.from(bytes, (b) => chars[b % chars.length]).join('')
}

/** 0–4: grobe Einschätzung, wie schwer das Passwort zu erraten ist */
function strength(pw: string) {
  let score = 0
  if (pw.length >= 8) score++
  if (pw.length >= 12) score++
  if (/[a-z]/.test(pw) && /[A-Z]/.test(pw)) score++
  if (/\d/.test(pw) && /[^a-zA-Z0-9]/.test(pw)) score++
  else if (/\d/.test(pw) || /[^a-zA-Z0-9]/.test(pw)) score += 0.5
  return Math.min(4, Math.floor(score))
}

const STRENGTH = [
  { label: 'sehr schwach', color: 'error' },
  { label: 'schwach', color: 'error' },
  { label: 'okay', color: 'warning' },
  { label: 'gut', color: 'success' },
  { label: 'sehr gut', color: 'success' },
] as const

function PasswordField({ value, onChange, label = 'Passwort' }: { value: string; onChange: (v: string) => void; label?: string }) {
  const [show, setShow] = useState(false)
  const s = strength(value)
  return (
    <Box>
      <TextField
        label={label}
        fullWidth
        type={show ? 'text' : 'password'}
        autoComplete="new-password"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        slotProps={{
          input: {
            sx: { fontFamily: show ? 'ui-monospace, monospace' : undefined },
            endAdornment: (
              <InputAdornment position="end">
                <Tooltip title={show ? 'Verbergen' : 'Anzeigen'}>
                  <IconButton onClick={() => setShow((v) => !v)} edge="end" aria-label={show ? 'Passwort verbergen' : 'Passwort anzeigen'}>
                    {show ? <VisibilityOffRoundedIcon /> : <VisibilityRoundedIcon />}
                  </IconButton>
                </Tooltip>
                <Tooltip title="Sicheres Passwort erzeugen">
                  <IconButton
                    onClick={() => {
                      onChange(generatePassword())
                      setShow(true)
                    }}
                    aria-label="Passwort erzeugen"
                  >
                    <CasinoRoundedIcon />
                  </IconButton>
                </Tooltip>
              </InputAdornment>
            ),
          },
        }}
      />
      {value && (
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1, mt: 0.75 }}>
          <LinearProgress
            variant="determinate"
            value={(s + 1) * 20}
            color={STRENGTH[s].color}
            sx={{ flex: 1, height: 5, borderRadius: 1 }}
          />
          <Typography variant="caption" color="text.secondary" sx={{ minWidth: 72 }}>
            {STRENGTH[s].label}
          </Typography>
        </Stack>
      )}
    </Box>
  )
}

/** Benutzer auflisten, anlegen, Passwort zurücksetzen, Rolle, Speicherlimit, löschen. */
export default function UsersPanel({ me, onToast }: Props) {
  const [users, setUsers] = useState<UserRow[]>([])
  const [storage, setStorage] = useState<Storage | null>(null)
  const [error, setError] = useState('')
  const [creating, setCreating] = useState(false)
  const [created, setCreated] = useState<{ name: string; password: string } | null>(null)
  const [menu, setMenu] = useState<{ user: UserRow; anchor: HTMLElement } | null>(null)
  const [resetFor, setResetFor] = useState<UserRow | null>(null)
  const [quotaFor, setQuotaFor] = useState<UserRow | null>(null)
  const [deleteFor, setDeleteFor] = useState<UserRow | null>(null)

  const load = useCallback(() => {
    api
      .users()
      .then((r) => setUsers(r.users))
      .catch((e) => setError((e as Error).message))
    api
      .storage()
      .then(setStorage)
      .catch(() => {})
  }, [])

  useEffect(() => {
    load()
  }, [load])

  const usage = useMemo(() => new Map((storage?.users ?? []).map((u) => [u.name, u.usage.bytes])), [storage])

  async function act(fn: () => Promise<unknown>, success: string) {
    setError('')
    try {
      await fn()
      onToast(success)
      load()
      return true
    } catch (e) {
      setError((e as Error).message)
      return false
    }
  }

  return (
    <>
      {error && (
        <Alert severity="error" sx={{ mb: 2 }} onClose={() => setError('')}>
          {error}
        </Alert>
      )}

      <Stack spacing={1}>
        {users.map((u) => {
          const used = usage.get(u.name) ?? 0
          return (
            <Stack
              key={u.name}
              direction="row"
              sx={{ alignItems: 'center', gap: 1.5, p: 1.5, borderRadius: 2, border: 1, borderColor: 'divider' }}
            >
              <Avatar
                sx={{
                  bgcolor: u.admin ? 'primary.main' : 'action.selected',
                  color: u.admin ? 'primary.contrastText' : 'text.primary',
                  fontWeight: 700,
                }}
              >
                {u.name[0].toUpperCase()}
              </Avatar>
              <Box sx={{ flex: 1, minWidth: 0 }}>
                <Stack direction="row" sx={{ gap: 1, alignItems: 'center', flexWrap: 'wrap' }}>
                  <Typography sx={{ fontWeight: 600 }}>{u.name}</Typography>
                  {u.name === me.name && <Chip label="Du" size="small" />}
                  {u.admin && <Chip label="Administrator" size="small" color="primary" variant="outlined" />}
                </Stack>
                <Typography variant="body2" color="text.secondary" noWrap>
                  {formatSize(used)} privat belegt
                  {u.quota_bytes ? ` von ${formatSize(u.quota_bytes)}` : ''}
                  {u.created_at ? ` · seit ${new Date(u.created_at * 1000).toLocaleDateString('de-DE')}` : ''}
                </Typography>
                {u.quota_bytes ? (
                  <LinearProgress
                    variant="determinate"
                    value={Math.min(100, (used / u.quota_bytes) * 100)}
                    color={used / u.quota_bytes > 0.9 ? 'error' : 'primary'}
                    sx={{ mt: 0.5, height: 4, borderRadius: 1, maxWidth: 240 }}
                  />
                ) : null}
              </Box>
              <IconButton onClick={(e) => setMenu({ user: u, anchor: e.currentTarget })} aria-label={`Aktionen für ${u.name}`}>
                <MoreVertIcon />
              </IconButton>
            </Stack>
          )
        })}
      </Stack>

      <Button variant="contained" startIcon={<PersonAddRoundedIcon />} onClick={() => setCreating(true)} sx={{ mt: 2 }}>
        Benutzer hinzufügen
      </Button>

      <Menu anchorEl={menu?.anchor} open={!!menu} onClose={() => setMenu(null)}>
        <MenuItem
          onClick={() => {
            setResetFor(menu!.user)
            setMenu(null)
          }}
        >
          <ListItemIcon>
            <KeyRoundedIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>Passwort zurücksetzen</ListItemText>
        </MenuItem>
        <MenuItem
          onClick={() => {
            setQuotaFor(menu!.user)
            setMenu(null)
          }}
        >
          <ListItemIcon>
            <StorageRoundedIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>Speicherlimit</ListItemText>
        </MenuItem>
        {menu && menu.user.name !== me.name && (
          <MenuItem
            onClick={() => {
              const u = menu.user
              setMenu(null)
              act(
                () => api.updateUser(u.name, { admin: !u.admin }),
                u.admin ? `${u.name} ist jetzt normaler Benutzer` : `${u.name} ist jetzt Administrator`,
              )
            }}
          >
            <ListItemIcon>
              {menu.user.admin ? <PersonRoundedIcon fontSize="small" /> : <AdminPanelSettingsRoundedIcon fontSize="small" />}
            </ListItemIcon>
            <ListItemText>{menu.user.admin ? 'Zum normalen Benutzer machen' : 'Zum Administrator machen'}</ListItemText>
          </MenuItem>
        )}
        {menu && menu.user.name !== me.name && (
          <MenuItem
            onClick={() => {
              setDeleteFor(menu.user)
              setMenu(null)
            }}
            sx={{ color: 'error.main' }}
          >
            <ListItemIcon>
              <DeleteOutlineRoundedIcon fontSize="small" color="error" />
            </ListItemIcon>
            <ListItemText>Löschen</ListItemText>
          </MenuItem>
        )}
      </Menu>

      <CreateDialog
        open={creating}
        existing={users.map((u) => u.name)}
        onClose={() => setCreating(false)}
        onCreated={(name, password, quotaError) => {
          setCreating(false)
          setCreated({ name, password })
          if (quotaError) onToast(`Benutzer angelegt, Speicherlimit aber nicht gespeichert: ${quotaError}`)
          load()
        }}
      />
      <CreatedDialog data={created} onClose={() => setCreated(null)} onToast={onToast} />

      <ResetDialog
        user={resetFor}
        onClose={() => setResetFor(null)}
        onSubmit={(pw) =>
          act(() => api.updateUser(resetFor!.name, { password: pw }), `Passwort von ${resetFor!.name} geändert`).then((ok) => {
            if (ok) {
              setCreated({ name: resetFor!.name, password: pw })
              setResetFor(null)
            }
          })
        }
      />
      <QuotaDialog
        user={quotaFor}
        onClose={() => setQuotaFor(null)}
        onSubmit={(bytes) =>
          act(() => api.updateUser(quotaFor!.name, { quota_bytes: bytes }), 'Speicherlimit gespeichert').then(
            (ok) => ok && setQuotaFor(null),
          )
        }
      />

      <Dialog open={!!deleteFor} onClose={() => setDeleteFor(null)}>
        <DialogTitle>Benutzer „{deleteFor?.name}“ löschen?</DialogTitle>
        <DialogContent>
          <DialogContentText>
            Der Benutzer kann sich danach weder im Web noch am Netzlaufwerk anmelden. Seine Alben, Favoriten und Teilen-Links werden
            gelöscht. Seine privaten Dateien bleiben auf dem Server im Ordner „Privat/{deleteFor?.name}“ erhalten.
          </DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setDeleteFor(null)}>Abbrechen</Button>
          <Button
            color="error"
            variant="contained"
            onClick={() => act(() => api.deleteUser(deleteFor!.name), `${deleteFor!.name} gelöscht`).then(() => setDeleteFor(null))}
          >
            Löschen
          </Button>
        </DialogActions>
      </Dialog>
    </>
  )
}

const NAME_RULE = /^[a-z][a-z0-9_-]{0,31}$/

function CreateDialog({
  open,
  existing,
  onClose,
  onCreated,
}: {
  open: boolean
  existing: string[]
  onClose: () => void
  onCreated: (name: string, password: string, quotaError?: string) => void
}) {
  const [name, setName] = useState('')
  const [password, setPassword] = useState('')
  const [admin, setAdmin] = useState(false)
  const [quota, setQuota] = useState('')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    if (open) {
      setName('')
      setPassword(generatePassword())
      setAdmin(false)
      setQuota('')
      setError('')
    }
  }, [open])

  const nameError =
    name && !NAME_RULE.test(name)
      ? 'Nur Kleinbuchstaben, Ziffern, _ und -, mit einem Buchstaben am Anfang'
      : existing.includes(name)
        ? 'Diesen Benutzer gibt es schon'
        : ''

  const gb = parseGb(quota)

  async function submit() {
    if (gb === null) return
    setBusy(true)
    setError('')
    try {
      await api.createUser(name, password, admin)
    } catch (e) {
      setError((e as Error).message)
      setBusy(false)
      return
    }
    // Benutzer gibt es jetzt: Passwort auf jeden Fall anzeigen, Limit-Fehler getrennt melden
    try {
      if (gb) await api.updateUser(name, { quota_bytes: Math.round(gb * GB) })
      onCreated(name, password)
    } catch (e) {
      onCreated(name, password, (e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  const roleCard = (value: boolean, icon: React.ReactNode, title: string, text: string) => (
    <ButtonBase
      onClick={() => setAdmin(value)}
      sx={{
        flex: 1,
        display: 'block',
        textAlign: 'left',
        p: 1.5,
        borderRadius: 2,
        border: 2,
        borderColor: admin === value ? 'primary.main' : 'divider',
        bgcolor: admin === value ? 'action.selected' : 'transparent',
      }}
    >
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, mb: 0.5 }}>
        {icon}
        <Typography sx={{ fontWeight: 600 }}>{title}</Typography>
        {admin === value && <CheckCircleRoundedIcon color="primary" fontSize="small" sx={{ ml: 'auto' }} />}
      </Stack>
      <Typography variant="body2" color="text.secondary">
        {text}
      </Typography>
    </ButtonBase>
  )

  return (
    <Dialog open={open} onClose={onClose} fullWidth maxWidth="sm">
      <form
        onSubmit={(e) => {
          e.preventDefault()
          if (name && !nameError && password.length >= 4 && gb !== null) submit()
        }}
      >
        <DialogTitle>Benutzer hinzufügen</DialogTitle>
        <DialogContent>
          <Stack spacing={2.5} sx={{ pt: 1 }}>
            {error && <Alert severity="error">{error}</Alert>}
            <TextField
              label="Benutzername"
              autoFocus
              autoComplete="off"
              autoCapitalize="none"
              value={name}
              onChange={(e) => setName(e.target.value.toLowerCase().replace(/\s/g, ''))}
              error={!!nameError}
              helperText={nameError || 'z. B. anna oder papa – damit meldet man sich an'}
            />
            <PasswordField value={password} onChange={setPassword} />
            <Stack direction={{ xs: 'column', sm: 'row' }} spacing={1.5}>
              {roleCard(false, <PersonRoundedIcon color="action" />, 'Benutzer', 'Eigene Fotos in „Privat“, Zugriff auf „Gemeinsam“.')}
              {roleCard(
                true,
                <AdminPanelSettingsRoundedIcon color="primary" />,
                'Administrator',
                'Zusätzlich Benutzer, Festplatten, Backup und Einstellungen verwalten.',
              )}
            </Stack>
            <TextField
              label="Speicherlimit für „Privat“ (optional)"
              placeholder="unbegrenzt"
              value={quota}
              onChange={(e) => setQuota(e.target.value)}
              error={gb === null}
              helperText={gb === null ? GB_ERROR : undefined}
              slotProps={{ input: { endAdornment: <InputAdornment position="end">GB</InputAdornment> } }}
            />
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={onClose}>Abbrechen</Button>
          <Button
            type="submit"
            variant="contained"
            startIcon={<PersonAddRoundedIcon />}
            disabled={busy || !name || !!nameError || password.length < 4 || gb === null}
          >
            Anlegen
          </Button>
        </DialogActions>
      </form>
    </Dialog>
  )
}

/** Nach dem Anlegen: Zugangsdaten zum Weitergeben, mit Kopieren-Knopf. */
function CreatedDialog({
  data,
  onClose,
  onToast,
}: {
  data: { name: string; password: string } | null
  onClose: () => void
  onToast: (m: string) => void
}) {
  const text = data
    ? `Dein Zugang zu ${location.hostname}:\n` +
      `Web: ${location.origin}\n` +
      `Netzlaufwerk: smb://${location.hostname} (Windows: \\\\${location.hostname}\\Gemeinsam)\n` +
      `Benutzername: ${data.name}\n` +
      `Passwort: ${data.password}`
    : ''
  return (
    <Dialog open={!!data} onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
          <CheckCircleRoundedIcon color="success" /> Zugangsdaten für {data?.name}
        </Stack>
      </DialogTitle>
      <DialogContent>
        <DialogContentText sx={{ mb: 2 }}>
          Gib diese Daten an {data?.name} weiter. Das Passwort wird nur jetzt angezeigt; es kann später in „Passwort ändern“ selbst geändert
          werden.
        </DialogContentText>
        <Box
          component="pre"
          sx={{
            m: 0,
            p: 2,
            borderRadius: 2,
            bgcolor: 'action.hover',
            fontFamily: 'ui-monospace, monospace',
            fontSize: 13,
            whiteSpace: 'pre-wrap',
          }}
        >
          {text}
        </Box>
      </DialogContent>
      <DialogActions>
        <Button startIcon={<ContentCopyRoundedIcon />} onClick={() => copyText(text).then(() => onToast('Zugangsdaten kopiert'))}>
          Kopieren
        </Button>
        <Button variant="contained" onClick={onClose}>
          Fertig
        </Button>
      </DialogActions>
    </Dialog>
  )
}

function ResetDialog({ user, onClose, onSubmit }: { user: User | null; onClose: () => void; onSubmit: (pw: string) => void }) {
  const [pw, setPw] = useState('')
  useEffect(() => setPw(user ? generatePassword() : ''), [user])
  return (
    <Dialog open={!!user} onClose={onClose} fullWidth maxWidth="xs">
      <form
        onSubmit={(e) => {
          e.preventDefault()
          if (pw.length >= 4) onSubmit(pw)
        }}
      >
        <DialogTitle>Neues Passwort für {user?.name}</DialogTitle>
        <DialogContent>
          <Box sx={{ pt: 1 }}>
            <PasswordField value={pw} onChange={setPw} label="Neues Passwort" />
          </Box>
          <Typography variant="body2" color="text.secondary" sx={{ mt: 1.5 }}>
            {user?.name} wird auf allen Geräten abgemeldet. Das Passwort gilt auch für das Netzlaufwerk.
          </Typography>
        </DialogContent>
        <DialogActions>
          <Button onClick={onClose}>Abbrechen</Button>
          <Button type="submit" variant="contained" disabled={pw.length < 4}>
            Speichern
          </Button>
        </DialogActions>
      </form>
    </Dialog>
  )
}

function QuotaDialog({ user, onClose, onSubmit }: { user: UserRow | null; onClose: () => void; onSubmit: (bytes: number) => void }) {
  const [gb, setGb] = useState('')
  useEffect(() => {
    if (user) setGb(user.quota_bytes ? String(Math.round((user.quota_bytes / GB) * 10) / 10) : '')
  }, [user])
  const value = parseGb(gb)
  return (
    <Dialog open={!!user} onClose={onClose} fullWidth maxWidth="xs">
      <form
        onSubmit={(e) => {
          e.preventDefault()
          if (value !== null) onSubmit(Math.round(value * GB))
        }}
      >
        <DialogTitle>Speicherlimit für {user?.name}</DialogTitle>
        <DialogContent>
          <TextField
            autoFocus
            fullWidth
            margin="dense"
            label="Limit für „Privat“"
            placeholder="leer = unbegrenzt"
            value={gb}
            onChange={(e) => setGb(e.target.value)}
            slotProps={{ input: { endAdornment: <InputAdornment position="end">GB</InputAdornment> } }}
            error={value === null}
            helperText={value === null ? GB_ERROR : 'Gilt für Uploads in den privaten Bereich. „Gemeinsam“ zählt nicht mit.'}
          />
        </DialogContent>
        <DialogActions>
          <Button onClick={onClose}>Abbrechen</Button>
          <Button type="submit" variant="contained" disabled={value === null}>
            Speichern
          </Button>
        </DialogActions>
      </form>
    </Dialog>
  )
}
