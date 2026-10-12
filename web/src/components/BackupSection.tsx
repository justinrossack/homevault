import { useCallback, useEffect, useState } from 'react'
import {
  Alert,
  Box,
  Button,
  Card,
  Checkbox,
  Chip,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  FormControlLabel,
  FormGroup,
  IconButton,
  LinearProgress,
  List,
  ListItem,
  ListItemText,
  MenuItem,
  Stack,
  Switch,
  TextField,
  Tooltip,
  Typography,
} from '@mui/material'
import AddRoundedIcon from '@mui/icons-material/AddRounded'
import EditRoundedIcon from '@mui/icons-material/EditRounded'
import DeleteOutlineRoundedIcon from '@mui/icons-material/DeleteOutlineRounded'
import HistoryRoundedIcon from '@mui/icons-material/HistoryRounded'
import PlayArrowRoundedIcon from '@mui/icons-material/PlayArrowRounded'
import StopRoundedIcon from '@mui/icons-material/StopRounded'
import NetworkCheckRoundedIcon from '@mui/icons-material/NetworkCheckRounded'
import RestoreRoundedIcon from '@mui/icons-material/RestoreRounded'
import RestoreDialog from './RestoreDialog'
import { api, formatSize, type BackupProgress, type BackupRun, type BackupTarget, type BackupTargetInput, type RestoreResult } from '../api'
import { useShell } from '../shell'

const WEEKDAYS = ['Montag', 'Dienstag', 'Mittwoch', 'Donnerstag', 'Freitag', 'Samstag', 'Sonntag']

const EMPTY: BackupTargetInput = {
  name: '',
  host: '',
  port: 445,
  share: '',
  folder: 'homevault',
  username: '',
  password: '',
  include_shared: true,
  include_private: true,
  include_db: true,
  schedule: 'daily',
  time: '03:00',
  weekday: 6,
  keep_days: 30,
  enabled: true,
  user_sync: false,
}

/** Ohne Bereiche dient das Ziel nur als Verbindung für den Live-Sync */
const hasAreas = (t: BackupTargetInput) => t.include_shared || t.include_private || t.include_db

const dateTime = (t: number) =>
  new Date(t * 1000).toLocaleString('de-DE', { day: '2-digit', month: '2-digit', year: 'numeric', hour: '2-digit', minute: '2-digit' })

function scheduleText(t: BackupTarget) {
  if (!t.enabled) return 'Pausiert'
  if (!hasAreas(t)) return 'Nur für Live-Sync'
  if (t.schedule === 'manual') return 'Nur manuell'
  if (t.schedule === 'weekly') return `Jeden ${WEEKDAYS[t.weekday]} um ${t.time} Uhr`
  return `Täglich um ${t.time} Uhr`
}

function StatusChip({ run }: { run?: BackupRun | null }) {
  if (!run) return <Chip size="small" label="Noch nie gesichert" />
  const map = {
    ok: { label: 'Erfolgreich', color: 'success' },
    error: { label: 'Fehler', color: 'error' },
    cancelled: { label: 'Abgebrochen', color: 'warning' },
    running: { label: 'Läuft', color: 'info' },
  } as const
  const s = map[run.status]
  return <Chip size="small" color={s.color} variant="outlined" label={s.label} />
}

/** Backup-Ziele verwalten (Einstellungen, nur Admin). */
export default function BackupSection() {
  const shell = useShell()
  const [targets, setTargets] = useState<BackupTarget[] | null>(null)
  const [running, setRunning] = useState<BackupProgress | null>(null)
  const [edit, setEdit] = useState<{ id?: number; value: BackupTargetInput; hasPassword: boolean } | null>(null)
  const [history, setHistory] = useState<BackupTarget | null>(null)
  const [toDelete, setToDelete] = useState<BackupTarget | null>(null)
  const [restoreFrom, setRestoreFrom] = useState<BackupTarget | null>(null)
  const [restoreResult, setRestoreResult] = useState<RestoreResult | null>(null)

  const load = useCallback(() => {
    api
      .backups()
      .then((r) => {
        setTargets(r.targets)
        setRunning(r.running)
        setRestoreResult(r.restore ?? null)
      })
      .catch((e) => shell.toast((e as Error).message))
  }, [shell])

  // schneller aktualisieren, solange ein Backup läuft
  useEffect(() => {
    load()
    const t = setInterval(load, running ? 1500 : 10000)
    return () => clearInterval(t)
  }, [load, !!running])

  async function act(fn: () => Promise<unknown>, msg?: string) {
    try {
      await fn()
      if (msg) shell.toast(msg)
      load()
    } catch (e) {
      shell.toast((e as Error).message)
    }
  }

  return (
    <Box>
      <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
        Sichert deine Daten auf andere NAS im Haus (Synology, QNAP, Fritzbox, Windows-Freigaben oder einen zweiten homevault). Es werden nur
        Änderungen übertragen. Gelöschte oder geänderte Dateien bleiben auf dem Ziel im Ordner <code>.versionen</code> erhalten, solange du
        es einstellst.
      </Typography>

      {restoreResult && !running && (
        <Alert severity={restoreResult.ok ? 'success' : 'error'} sx={{ mb: 2 }} onClose={() => setRestoreResult(null)}>
          {restoreResult.ok
            ? `Wiederherstellung abgeschlossen: ${restoreResult.files} Dateien (${formatSize(restoreResult.bytes ?? 0)})${restoreResult.folder ? ` im Ordner „${restoreResult.folder}“` : ' am ursprünglichen Ort'}.`
            : `Wiederherstellung fehlgeschlagen: ${restoreResult.message}`}
        </Alert>
      )}
      {targets === null ? (
        <CircularProgress size={24} />
      ) : (
        <Stack spacing={1.5} sx={{ mb: 2 }}>
          {targets.map((t) => {
            const live = running?.target_id === t.id ? running : null
            return (
              <Card key={t.id} variant="outlined" sx={{ p: 2, opacity: t.enabled ? 1 : 0.6 }}>
                <Stack direction="row" sx={{ alignItems: 'flex-start', gap: 1 }}>
                  <Box sx={{ flex: 1, minWidth: 0 }}>
                    <Stack direction="row" sx={{ alignItems: 'center', gap: 1, flexWrap: 'wrap' }}>
                      <Typography sx={{ fontWeight: 600 }}>{t.name}</Typography>
                      {live ? <Chip size="small" color="info" label="Läuft …" /> : <StatusChip run={t.last_run} />}
                    </Stack>
                    <Typography variant="body2" color="text.secondary" noWrap sx={{ fontFamily: 'ui-monospace, monospace', fontSize: 13 }}>
                      {t.location}
                    </Typography>
                    <Typography variant="body2" color="text.secondary">
                      {scheduleText(t)}
                      {t.next_run && t.schedule !== 'manual' ? ` · nächstes: ${dateTime(t.next_run)}` : ''}
                    </Typography>
                    {t.last_success && (
                      <Typography variant="body2" color="text.secondary">
                        Zuletzt erfolgreich: {dateTime(t.last_success)}
                      </Typography>
                    )}
                    {t.last_run?.status === 'error' && !live && (
                      <Alert severity="error" sx={{ mt: 1, py: 0 }}>
                        {t.last_run.message}
                      </Alert>
                    )}
                  </Box>
                  {live ? (
                    <Tooltip title="Abbrechen">
                      <IconButton onClick={() => act(api.cancelBackup, 'Backup wird abgebrochen')}>
                        <StopRoundedIcon />
                      </IconButton>
                    </Tooltip>
                  ) : (
                    <Tooltip title="Jetzt sichern">
                      <span>
                        <IconButton
                          color="primary"
                          disabled={!!running || !hasAreas(t)}
                          onClick={() => act(() => api.runBackup(t.id), 'Backup gestartet')}
                        >
                          <PlayArrowRoundedIcon />
                        </IconButton>
                      </span>
                    </Tooltip>
                  )}
                  <Tooltip title="Dateien aus diesem Backup wiederherstellen">
                    <span>
                      <IconButton disabled={!!running || !t.last_success} onClick={() => setRestoreFrom(t)}>
                        <RestoreRoundedIcon />
                      </IconButton>
                    </span>
                  </Tooltip>
                  <Tooltip title="Verlauf">
                    <IconButton onClick={() => setHistory(t)}>
                      <HistoryRoundedIcon />
                    </IconButton>
                  </Tooltip>
                  <Tooltip title="Bearbeiten">
                    <IconButton onClick={() => setEdit({ id: t.id, value: { ...t, password: '' }, hasPassword: t.has_password })}>
                      <EditRoundedIcon />
                    </IconButton>
                  </Tooltip>
                  <Tooltip title="Entfernen">
                    <span>
                      <IconButton disabled={!!live} onClick={() => setToDelete(t)}>
                        <DeleteOutlineRoundedIcon />
                      </IconButton>
                    </span>
                  </Tooltip>
                </Stack>
                {live && <LiveProgress p={live} />}
              </Card>
            )
          })}
        </Stack>
      )}

      <Button variant="outlined" startIcon={<AddRoundedIcon />} onClick={() => setEdit({ value: EMPTY, hasPassword: false })}>
        Backup-Ziel hinzufügen
      </Button>

      <TargetDialog
        state={edit}
        onClose={() => setEdit(null)}
        onSaved={(msg) => {
          setEdit(null)
          shell.toast(msg)
          load()
        }}
      />
      <HistoryDialog target={history} onClose={() => setHistory(null)} />
      <RestoreDialog
        target={restoreFrom}
        onClose={() => setRestoreFrom(null)}
        onStarted={() => {
          setRestoreFrom(null)
          shell.toast('Wiederherstellung gestartet')
          load()
        }}
      />
      <Dialog open={!!toDelete} onClose={() => setToDelete(null)}>
        <DialogTitle>„{toDelete?.name}“ entfernen?</DialogTitle>
        <DialogContent>
          <DialogContentText>
            Das Ziel wird aus homevault entfernt. Die bereits gesicherten Dateien auf {toDelete?.location} bleiben unangetastet.
          </DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setToDelete(null)}>Abbrechen</Button>
          <Button
            color="error"
            variant="contained"
            onClick={() => {
              const t = toDelete!
              setToDelete(null)
              act(() => api.deleteBackup(t.id), 'Backup-Ziel entfernt')
            }}
          >
            Entfernen
          </Button>
        </DialogActions>
      </Dialog>
    </Box>
  )
}

function LiveProgress({ p }: { p: BackupProgress }) {
  const fraction = p.total_bytes > 0 ? (p.bytes / p.total_bytes) * 100 : undefined
  return (
    <Box sx={{ mt: 1.5 }}>
      <LinearProgress
        variant={fraction === undefined ? 'indeterminate' : 'determinate'}
        value={fraction}
        sx={{ borderRadius: 1, mb: 0.5 }}
      />
      <Typography variant="caption" color="text.secondary">
        {p.part || 'Startet'}
        {p.total_transfers > 0 && ` · ${p.transfers} von ${p.total_transfers} Dateien`}
        {p.total_bytes > 0 && ` · ${formatSize(p.bytes)} von ${formatSize(p.total_bytes)}`}
        {p.speed > 0 && ` · ${formatSize(p.speed)}/s`}
      </Typography>
    </Box>
  )
}

function TargetDialog({
  state,
  onClose,
  onSaved,
}: {
  state: { id?: number; value: BackupTargetInput; hasPassword: boolean } | null
  onClose: () => void
  onSaved: (msg: string) => void
}) {
  const [v, setV] = useState<BackupTargetInput>(EMPTY)
  const [test, setTest] = useState<{ ok: boolean; message: string } | null>(null)
  const [busy, setBusy] = useState<'test' | 'save' | null>(null)
  const [error, setError] = useState('')

  useEffect(() => {
    if (state) {
      setV(state.value)
      setTest(null)
      setError('')
    }
  }, [state])

  const set = <K extends keyof BackupTargetInput>(k: K, value: BackupTargetInput[K]) => setV((p) => ({ ...p, [k]: value }))
  const needsPassword = !state?.hasPassword
  const connection = v.name.trim() && v.host.trim() && v.share.trim() && v.username.trim() && (!needsPassword || v.password)
  // ohne Bereiche nur mit Zeitplan „Nur manuell“ (reine Verbindung für den Live-Sync)
  const areasOk = hasAreas(v) || v.schedule === 'manual'
  const complete = connection && areasOk

  async function runTest() {
    setBusy('test')
    setTest(null)
    setError('')
    try {
      setTest(await api.testBackup({ ...v, name: v.name || 'Test' }, state?.id))
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusy(null)
    }
  }

  async function save() {
    setBusy('save')
    setError('')
    try {
      if (state?.id) await api.updateBackup(state.id, v)
      else await api.createBackup(v)
      onSaved(state?.id ? 'Gespeichert' : `Backup-Ziel „${v.name}“ angelegt`)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusy(null)
    }
  }

  return (
    <Dialog open={!!state} onClose={onClose} fullWidth maxWidth="sm">
      <form
        onSubmit={(e) => {
          e.preventDefault()
          if (complete) save()
        }}
      >
        <DialogTitle>{state?.id ? 'Backup-Ziel bearbeiten' : 'Neues Backup-Ziel'}</DialogTitle>
        <DialogContent>
          <Stack spacing={2} sx={{ pt: 1 }}>
            {error && <Alert severity="error">{error}</Alert>}
            <TextField
              label="Name"
              placeholder="z. B. Papas Synology"
              value={v.name}
              onChange={(e) => set('name', e.target.value)}
              autoFocus
            />

            <Typography variant="subtitle2">Verbindung (SMB)</Typography>
            <Stack direction={{ xs: 'column', sm: 'row' }} spacing={1.5}>
              <TextField
                label="Server"
                placeholder="192.168.1.20 oder nas.local"
                value={v.host}
                onChange={(e) => set('host', e.target.value)}
                sx={{ flex: 2 }}
              />
              <TextField
                label="Port"
                type="number"
                value={v.port}
                onChange={(e) => set('port', Number(e.target.value))}
                sx={{ flex: 1, maxWidth: { sm: 110 } }}
              />
            </Stack>
            <Stack direction={{ xs: 'column', sm: 'row' }} spacing={1.5}>
              <TextField
                label="Freigabe"
                placeholder="z. B. backup"
                value={v.share}
                onChange={(e) => set('share', e.target.value)}
                sx={{ flex: 1 }}
              />
              <TextField
                label="Ordner auf der Freigabe"
                value={v.folder}
                onChange={(e) => set('folder', e.target.value)}
                helperText="Wird angelegt, falls nötig"
                sx={{ flex: 1 }}
              />
            </Stack>
            <Stack direction={{ xs: 'column', sm: 'row' }} spacing={1.5}>
              <TextField
                label="Benutzer auf dem Ziel"
                autoComplete="off"
                value={v.username}
                onChange={(e) => set('username', e.target.value)}
                sx={{ flex: 1 }}
              />
              <TextField
                label="Passwort"
                type="password"
                autoComplete="new-password"
                value={v.password}
                onChange={(e) => set('password', e.target.value)}
                placeholder={needsPassword ? '' : 'unverändert'}
                slotProps={{ inputLabel: { shrink: needsPassword ? undefined : true } }}
                sx={{ flex: 1 }}
              />
            </Stack>
            <Box>
              <Button
                variant="outlined"
                startIcon={busy === 'test' ? <CircularProgress size={16} /> : <NetworkCheckRoundedIcon />}
                disabled={!connection || busy !== null}
                onClick={runTest}
              >
                Verbindung testen
              </Button>
              {test && (
                <Alert severity={test.ok ? 'success' : 'error'} sx={{ mt: 1.5 }}>
                  {test.message}
                </Alert>
              )}
            </Box>

            <Typography variant="subtitle2" sx={{ pt: 1 }}>
              Was sichern?
            </Typography>
            <FormGroup>
              <FormControlLabel
                control={<Checkbox checked={v.include_shared} onChange={(e) => set('include_shared', e.target.checked)} />}
                label="Gemeinsam"
              />
              <FormControlLabel
                control={<Checkbox checked={v.include_private} onChange={(e) => set('include_private', e.target.checked)} />}
                label="Privat-Ordner aller Benutzer"
              />
              <FormControlLabel
                control={<Checkbox checked={v.include_db} onChange={(e) => set('include_db', e.target.checked)} />}
                label="Datenbank (Benutzer, Alben, Favoriten)"
              />
            </FormGroup>
            {!hasAreas(v) && (
              <Alert severity={areasOk ? 'info' : 'warning'}>
                {areasOk
                  ? 'Es wird nichts gesichert – das NAS dient nur als Ziel für den Live-Sync.'
                  : 'Bitte mindestens einen Bereich auswählen oder den Zeitplan „Nur manuell“ wählen, wenn das NAS nur für den Live-Sync dienen soll.'}
              </Alert>
            )}

            <Typography variant="subtitle2" sx={{ pt: 1 }}>
              Wann?
            </Typography>
            <Stack direction={{ xs: 'column', sm: 'row' }} spacing={1.5}>
              <TextField
                select
                label="Zeitplan"
                value={v.schedule}
                onChange={(e) => set('schedule', e.target.value as BackupTargetInput['schedule'])}
                sx={{ flex: 1 }}
              >
                <MenuItem value="daily">Täglich</MenuItem>
                <MenuItem value="weekly">Wöchentlich</MenuItem>
                <MenuItem value="manual">Nur manuell</MenuItem>
              </TextField>
              {v.schedule === 'weekly' && (
                <TextField select label="Tag" value={v.weekday} onChange={(e) => set('weekday', Number(e.target.value))} sx={{ flex: 1 }}>
                  {WEEKDAYS.map((d, i) => (
                    <MenuItem key={d} value={i}>
                      {d}
                    </MenuItem>
                  ))}
                </TextField>
              )}
              {v.schedule !== 'manual' && (
                <TextField label="Uhrzeit" type="time" value={v.time} onChange={(e) => set('time', e.target.value)} sx={{ flex: 1 }} />
              )}
            </Stack>
            <TextField
              select
              label="Gelöschte und geänderte Dateien aufbewahren"
              value={v.keep_days}
              onChange={(e) => set('keep_days', Number(e.target.value))}
            >
              <MenuItem value={7}>7 Tage</MenuItem>
              <MenuItem value={30}>30 Tage</MenuItem>
              <MenuItem value={90}>90 Tage</MenuItem>
              <MenuItem value={365}>1 Jahr</MenuItem>
              <MenuItem value={0}>Für immer</MenuItem>
            </TextField>
            <FormControlLabel control={<Switch checked={v.enabled} onChange={(e) => set('enabled', e.target.checked)} />} label="Aktiv" />

            <Typography variant="subtitle2" sx={{ pt: 1 }}>
              Live-Sync
            </Typography>
            <Box>
              <FormControlLabel
                control={<Switch checked={v.user_sync} onChange={(e) => set('user_sync', e.target.checked)} />}
                label="Benutzer dürfen ihren Privat-Ordner auf dieses NAS synchronisieren"
              />
              <Typography variant="body2" color="text.secondary" sx={{ ml: 6 }}>
                Jeder bekommt dort einen eigenen Ordner: {v.folder.trim() ? `${v.folder.trim()}/` : ''}Benutzer/&lt;Name&gt;
              </Typography>
            </Box>
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={onClose}>Abbrechen</Button>
          <Button type="submit" variant="contained" disabled={!complete || busy !== null}>
            Speichern
          </Button>
        </DialogActions>
      </form>
    </Dialog>
  )
}

function HistoryDialog({ target, onClose }: { target: BackupTarget | null; onClose: () => void }) {
  const [runs, setRuns] = useState<BackupRun[] | null>(null)
  useEffect(() => {
    if (!target) return
    setRuns(null)
    api
      .backupRuns(target.id)
      .then((r) => setRuns(r.runs))
      .catch(() => setRuns([]))
  }, [target])

  return (
    <Dialog open={!!target} onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>Verlauf: {target?.name}</DialogTitle>
      <DialogContent>
        {runs === null ? (
          <CircularProgress size={24} />
        ) : runs.length === 0 ? (
          <Typography color="text.secondary">Noch keine Backups.</Typography>
        ) : (
          <List dense>
            {runs.map((r) => (
              <ListItem key={r.started_at} disableGutters>
                <ListItemText
                  primary={
                    <Stack direction="row" sx={{ gap: 1, alignItems: 'center' }}>
                      {dateTime(r.started_at)}
                      <StatusChip run={r} />
                    </Stack>
                  }
                  secondary={
                    r.status === 'ok'
                      ? `${r.files} Dateien übertragen (${formatSize(r.bytes)}) in ${Math.max(1, (r.finished_at ?? r.started_at) - r.started_at)} s`
                      : r.message
                  }
                />
              </ListItem>
            ))}
          </List>
        )}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Schließen</Button>
      </DialogActions>
    </Dialog>
  )
}
