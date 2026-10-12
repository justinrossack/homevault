import { useCallback, useEffect, useState } from 'react'
import {
  Alert,
  Box,
  Button,
  Card,
  Chip,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  IconButton,
  MenuItem,
  Stack,
  Switch,
  TextField,
  Tooltip,
  Typography,
} from '@mui/material'
import AddRoundedIcon from '@mui/icons-material/AddRounded'
import ArrowForwardRoundedIcon from '@mui/icons-material/ArrowForwardRounded'
import CheckCircleRoundedIcon from '@mui/icons-material/CheckCircleRounded'
import DeleteOutlineRoundedIcon from '@mui/icons-material/DeleteOutlineRounded'
import ErrorOutlineRoundedIcon from '@mui/icons-material/ErrorOutlineRounded'
import HourglassTopRoundedIcon from '@mui/icons-material/HourglassTopRounded'
import NewReleasesRoundedIcon from '@mui/icons-material/NewReleasesRounded'
import PauseCircleRoundedIcon from '@mui/icons-material/PauseCircleRounded'
import SyncRoundedIcon from '@mui/icons-material/SyncRounded'
import { ApiError, api, formatSize, type SyncRule, type SyncRuleInput, type SyncTarget } from '../api'
import { navigate } from '../router'
import { useShell } from '../shell'

const KEEP_OPTIONS = [
  { value: 7, label: '7 Tage' },
  { value: 30, label: '30 Tage' },
  { value: 90, label: '90 Tage' },
  { value: 365, label: '1 Jahr' },
  { value: 0, label: 'Für immer' },
]

const NOT_EMPTY = 'DEST_NOT_EMPTY:'

const dateTime = (t: number) =>
  new Date(t * 1000).toLocaleString('de-DE', { day: '2-digit', month: '2-digit', year: 'numeric', hour: '2-digit', minute: '2-digit' })

function ago(t: number) {
  const s = Math.max(0, Date.now() / 1000 - t)
  if (s < 60) return 'gerade eben'
  if (s < 3600) return `vor ${Math.floor(s / 60)} Min.`
  if (s < 86400) return `vor ${Math.floor(s / 3600)} Std.`
  const d = Math.floor(s / 86400)
  return d === 1 ? 'vor 1 Tag' : `vor ${d} Tagen`
}

const keepText = (days: number) => (days === 0 ? 'für immer' : days === 365 ? '1 Jahr' : `${days} Tage`)

function StatusChip({ rule }: { rule: SyncRule }) {
  const chip = {
    ok: {
      label: rule.last_ok_at ? `Aktuell · ${ago(rule.last_ok_at)}` : 'Aktuell',
      color: 'success',
      icon: <CheckCircleRoundedIcon />,
      tip: rule.last_ok_at ? `Zuletzt abgeglichen: ${dateTime(rule.last_ok_at)}` : '',
    },
    pending: {
      label: 'Gleich …',
      color: 'info',
      icon: <HourglassTopRoundedIcon />,
      tip: 'Änderungen erkannt – wird übertragen, sobald etwa eine Minute lang nichts mehr geändert wurde',
    },
    running: { label: 'Läuft …', color: 'info', icon: <CircularProgress size={14} color="inherit" />, tip: '' },
    error: { label: 'Fehler', color: 'error', icon: <ErrorOutlineRoundedIcon />, tip: 'Wird in einigen Minuten erneut versucht' },
    paused: { label: 'Pausiert', color: 'default', icon: <PauseCircleRoundedIcon />, tip: '' },
    new: { label: 'Neu', color: 'default', icon: <NewReleasesRoundedIcon />, tip: 'Noch nicht übertragen' },
  } as const
  const s = chip[rule.status] ?? chip.new
  return (
    <Tooltip title={s.tip}>
      <Chip size="small" variant="outlined" color={s.color} icon={s.icon} label={s.label} />
    </Tooltip>
  )
}

/** Live-Sync-Regeln (Einstellungen, für alle Benutzer). */
export default function SyncSection() {
  const shell = useShell()
  const admin = shell.user.admin
  const [data, setData] = useState<{ rules: SyncRule[]; targets: SyncTarget[]; users: string[] } | null>(null)
  const [adding, setAdding] = useState(false)
  const [toDelete, setToDelete] = useState<SyncRule | null>(null)

  const load = useCallback(() => {
    api
      .sync()
      .then(setData)
      .catch((e) => shell.toast((e as Error).message))
  }, [shell])

  // alle 5 s aktualisieren, solange die Seite sichtbar ist
  useEffect(() => {
    load()
    const t = setInterval(() => !document.hidden && load(), 5000)
    const onVisible = () => !document.hidden && load()
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      clearInterval(t)
      document.removeEventListener('visibilitychange', onVisible)
    }
  }, [load])

  async function act(fn: () => Promise<unknown>, msg?: string) {
    try {
      await fn()
      if (msg) shell.toast(msg)
      load()
    } catch (e) {
      shell.toast((e as Error).message)
    }
  }

  const noTargets = data !== null && data.targets.length === 0

  return (
    <Box>
      <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
        Ordner werden laufend auf andere NAS im Haus gespiegelt – etwa eine Minute nach jeder Änderung. Nur in eine Richtung: homevault →
        NAS. Gelöschtes bleibt auf dem NAS unter <code>.versionen</code>.
      </Typography>

      {data === null ? (
        <CircularProgress size={24} />
      ) : data.rules.length === 0 ? (
        <Stack sx={{ alignItems: 'center', textAlign: 'center', gap: 1, py: 4, color: 'text.secondary' }}>
          <SyncRoundedIcon sx={{ fontSize: 48, opacity: 0.5 }} />
          <Typography>Noch keine Ordner werden synchronisiert.</Typography>
        </Stack>
      ) : (
        <Stack spacing={1.5} sx={{ mb: 2 }}>
          {data.rules.map((r) => (
            <RuleCard
              key={r.id}
              rule={r}
              onRun={() => act(() => api.runSync(r.id), 'Wird synchronisiert')}
              onToggle={(enabled) => act(() => api.updateSync(r.id, { enabled }), enabled ? 'Sync läuft wieder' : 'Sync pausiert')}
              onDelete={() => setToDelete(r)}
            />
          ))}
        </Stack>
      )}

      {noTargets ? (
        admin ? (
          <Alert
            severity="info"
            action={
              <Button color="inherit" size="small" onClick={() => navigate({ view: 'settings', tab: 'backup' })}>
                NAS-Ziel unter Backup anlegen
              </Button>
            }
          >
            Es ist noch kein NAS eingerichtet. Lege es unter Backup an – ohne Sicherungsbereiche und mit Zeitplan „Nur manuell“ dient es nur
            als Ziel für den Live-Sync.
          </Alert>
        ) : (
          <Alert severity="info">Der Admin hat noch kein NAS für dich freigegeben.</Alert>
        )
      ) : (
        data && (
          <Button variant="outlined" startIcon={<AddRoundedIcon />} onClick={() => setAdding(true)}>
            Regel hinzufügen
          </Button>
        )
      )}

      {data && (
        <RuleDialog
          open={adding}
          targets={data.targets}
          users={data.users}
          onClose={() => setAdding(false)}
          onCreated={() => {
            setAdding(false)
            shell.toast('Regel angelegt – der erste Abgleich startet gleich')
            load()
          }}
        />
      )}

      <Dialog open={!!toDelete} onClose={() => setToDelete(null)}>
        <DialogTitle>Sync entfernen?</DialogTitle>
        <DialogContent>
          <DialogContentText>
            „{toDelete?.source_label}“ wird nicht mehr nach „{toDelete?.target_name}“ synchronisiert. Die Dateien auf {toDelete?.location}{' '}
            bleiben erhalten.
          </DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setToDelete(null)}>Abbrechen</Button>
          <Button
            color="error"
            variant="contained"
            onClick={() => {
              const r = toDelete!
              setToDelete(null)
              act(() => api.deleteSync(r.id), 'Sync entfernt')
            }}
          >
            Entfernen
          </Button>
        </DialogActions>
      </Dialog>
    </Box>
  )
}

function RuleCard({
  rule: r,
  onRun,
  onToggle,
  onDelete,
}: {
  rule: SyncRule
  onRun: () => void
  onToggle: (enabled: boolean) => void
  onDelete: () => void
}) {
  const busy = r.status === 'running' || r.status === 'pending'
  return (
    <Card variant="outlined" sx={{ p: 2, opacity: r.enabled ? 1 : 0.6 }}>
      <Stack direction="row" sx={{ alignItems: 'flex-start', gap: 1 }}>
        <Box sx={{ flex: 1, minWidth: 0 }}>
          <Stack direction="row" sx={{ alignItems: 'center', gap: 1, flexWrap: 'wrap' }}>
            <Typography sx={{ fontWeight: 600 }}>{r.source_label}</Typography>
            <ArrowForwardRoundedIcon fontSize="small" color="action" />
            <Typography sx={{ fontWeight: 600 }}>{r.target_name}</Typography>
            <StatusChip rule={r} />
          </Stack>
          <Typography variant="body2" color="text.secondary" noWrap sx={{ fontFamily: 'ui-monospace, monospace', fontSize: 13 }}>
            {r.location}
          </Typography>
          <Typography variant="body2" color="text.secondary">
            {r.last_run_at
              ? `Letzter Abgleich ${dateTime(r.last_run_at)}: ${r.files ? `${r.files.toLocaleString('de-DE')} Dateien (${formatSize(r.bytes)}) übertragen` : 'keine Änderungen'}`
              : 'Noch nicht abgeglichen'}
            {` · Gelöschtes ${keepText(r.keep_days)} aufbewahren`}
          </Typography>
          {r.by_admin && !r.can_edit && (
            <Typography variant="caption" color="text.secondary">
              vom Admin eingerichtet
            </Typography>
          )}
          {r.status === 'error' && r.last_error && (
            <Alert severity="error" sx={{ mt: 1, py: 0 }}>
              {r.last_error}
            </Alert>
          )}
        </Box>
        <Tooltip title="Jetzt synchronisieren">
          <span>
            <IconButton color="primary" disabled={!r.enabled || busy} onClick={onRun}>
              <SyncRoundedIcon />
            </IconButton>
          </span>
        </Tooltip>
        {r.can_edit && (
          <>
            <Tooltip title={r.enabled ? 'Pausieren' : 'Fortsetzen'}>
              <Switch checked={r.enabled} onChange={(e) => onToggle(e.target.checked)} />
            </Tooltip>
            <Tooltip title="Entfernen">
              <IconButton onClick={onDelete}>
                <DeleteOutlineRoundedIcon />
              </IconButton>
            </Tooltip>
          </>
        )}
      </Stack>
    </Card>
  )
}

/** Vorschlag für den Zielordner auf dem NAS (nur Admin) */
function suggestDest(source: string, sub: string) {
  const base = source === 'shared' ? 'sync/gemeinsam' : `sync/${source.slice('private:'.length)}`
  const s = sub
    .trim()
    .replace(/\\/g, '/')
    .replace(/^\/+|\/+$/g, '')
  return s ? `${base}/${s}` : base
}

function RuleDialog({
  open,
  targets,
  users,
  onClose,
  onCreated,
}: {
  open: boolean
  targets: SyncTarget[]
  users: string[]
  onClose: () => void
  onCreated: () => void
}) {
  const shell = useShell()
  const me = shell.user.name
  const admin = shell.user.admin
  // 'shared' oder 'private:<name>'
  const [source, setSource] = useState('shared')
  const [sub, setSub] = useState('')
  const [targetId, setTargetId] = useState<number | ''>('')
  const [dest, setDest] = useState('')
  const [destTouched, setDestTouched] = useState(false)
  const [keepDays, setKeepDays] = useState(30)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [confirm, setConfirm] = useState<string | null>(null)

  useEffect(() => {
    if (!open) return
    setSource(admin ? 'shared' : `private:${me}`)
    setSub('')
    setTargetId(targets.length === 1 ? targets[0].id : '')
    setDest(admin ? 'sync/gemeinsam' : '')
    setDestTouched(false)
    setKeepDays(30)
    setError('')
    setConfirm(null)
  }, [open])

  // Zielordner mitführen, solange er nicht von Hand geändert wurde
  useEffect(() => {
    if (admin && !destTouched) setDest(suggestDest(source, sub))
  }, [source, sub, destTouched, admin])

  const target = targets.find((t) => t.id === targetId)
  const cleanSub = sub
    .trim()
    .replace(/\\/g, '/')
    .replace(/^\/+|\/+$/g, '')
  const userDest = target ? `${target.folder ? `${target.folder}/` : ''}Benutzer/${me}${cleanSub ? `/${cleanSub}` : ''}` : ''
  const complete = targetId !== '' && (!admin || dest.trim())

  async function save(confirmNonempty = false) {
    if (targetId === '') return
    setBusy(true)
    setError('')
    const body: SyncRuleInput = { target_id: targetId, source: 'private', sub: cleanSub, keep_days: keepDays }
    if (admin) {
      if (source === 'shared') body.source = 'shared'
      else body.user = source.slice('private:'.length)
      body.dest = dest.trim()
    }
    if (confirmNonempty) body.confirm_nonempty = true
    try {
      await api.createSync(body)
      setConfirm(null)
      onCreated()
    } catch (e) {
      const msg = (e as Error).message
      if (e instanceof ApiError && e.status === 409 && msg.startsWith(NOT_EMPTY)) setConfirm(msg.slice(NOT_EMPTY.length).trim())
      else {
        setConfirm(null)
        setError(msg)
      }
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      <Dialog open={open} onClose={onClose} fullWidth maxWidth="sm">
        <form
          onSubmit={(e) => {
            e.preventDefault()
            if (complete && !busy) save()
          }}
        >
          <DialogTitle>Ordner synchronisieren</DialogTitle>
          <DialogContent>
            <Stack spacing={2} sx={{ pt: 1 }}>
              {error && <Alert severity="error">{error}</Alert>}
              <Typography variant="subtitle2">Quelle in homevault</Typography>
              {admin ? (
                <TextField select label="Ordner" value={source} onChange={(e) => setSource(e.target.value)}>
                  <MenuItem value="shared">Gemeinsam</MenuItem>
                  {users.map((u) => (
                    <MenuItem key={u} value={`private:${u}`}>
                      Privat von {u}
                      {u === me ? ' (du)' : ''}
                    </MenuItem>
                  ))}
                </TextField>
              ) : (
                <TextField label="Ordner" value="Mein Privat-Ordner" slotProps={{ input: { readOnly: true } }} />
              )}
              <TextField
                label="Unterordner (optional)"
                placeholder="z. B. Fotos/2024"
                value={sub}
                onChange={(e) => setSub(e.target.value)}
                helperText="leer = ganzer Ordner"
              />

              <Typography variant="subtitle2" sx={{ pt: 1 }}>
                Ziel
              </Typography>
              <TextField
                select
                label="NAS"
                value={targetId}
                onChange={(e) => setTargetId(Number(e.target.value))}
                helperText={target?.location}
              >
                {targets.map((t) => (
                  <MenuItem key={t.id} value={t.id}>
                    {t.name}
                  </MenuItem>
                ))}
              </TextField>
              {admin ? (
                <TextField
                  label="Zielordner auf dem NAS"
                  required
                  value={dest}
                  onChange={(e) => {
                    setDest(e.target.value)
                    setDestTouched(true)
                  }}
                  helperText={
                    target && dest.trim()
                      ? `${target.location}\\${dest.trim().replace(/\//g, '\\')}`
                      : 'Ordner innerhalb der Freigabe, wird angelegt, falls nötig'
                  }
                />
              ) : (
                target && (
                  <TextField
                    label="Zielordner auf dem NAS"
                    value={userDest}
                    slotProps={{ input: { readOnly: true } }}
                    helperText="Dein eigener Ordner auf diesem NAS"
                  />
                )
              )}
              <TextField
                select
                label="Gelöschte und geänderte Dateien aufbewahren"
                value={keepDays}
                onChange={(e) => setKeepDays(Number(e.target.value))}
              >
                {KEEP_OPTIONS.map((o) => (
                  <MenuItem key={o.value} value={o.value}>
                    {o.label}
                  </MenuItem>
                ))}
              </TextField>
            </Stack>
          </DialogContent>
          <DialogActions>
            <Button onClick={onClose}>Abbrechen</Button>
            <Button
              type="submit"
              variant="contained"
              disabled={!complete || busy}
              startIcon={busy && !confirm ? <CircularProgress size={16} color="inherit" /> : undefined}
            >
              Synchronisieren
            </Button>
          </DialogActions>
        </form>
      </Dialog>

      <Dialog open={confirm !== null} onClose={() => setConfirm(null)}>
        <DialogTitle>Zielordner ist nicht leer</DialogTitle>
        <DialogContent>
          <DialogContentText>{confirm}</DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setConfirm(null)}>Abbrechen</Button>
          <Button
            variant="contained"
            disabled={busy}
            startIcon={busy ? <CircularProgress size={16} color="inherit" /> : undefined}
            onClick={() => save(true)}
          >
            Trotzdem synchronisieren
          </Button>
        </DialogActions>
      </Dialog>
    </>
  )
}
