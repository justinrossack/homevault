import { useCallback, useEffect, useRef, useState } from 'react'
import {
  Alert,
  AlertTitle,
  Box,
  Button,
  Checkbox,
  Chip,
  CircularProgress,
  Collapse,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  LinearProgress,
  MenuItem,
  Stack,
  TextField,
  Typography,
} from '@mui/material'
import StorageRoundedIcon from '@mui/icons-material/StorageRounded'
import SdStorageRoundedIcon from '@mui/icons-material/SdStorageRounded'
import CheckCircleRoundedIcon from '@mui/icons-material/CheckCircleRounded'
import ErrorRoundedIcon from '@mui/icons-material/ErrorRounded'
import WarningAmberRoundedIcon from '@mui/icons-material/WarningAmberRounded'
import LockRoundedIcon from '@mui/icons-material/LockRounded'
import AddRoundedIcon from '@mui/icons-material/AddRounded'
import SwapHorizRoundedIcon from '@mui/icons-material/SwapHorizRounded'
import RemoveCircleOutlineRoundedIcon from '@mui/icons-material/RemoveCircleOutlineRounded'
import FactCheckRoundedIcon from '@mui/icons-material/FactCheckRounded'
import ExpandMoreRoundedIcon from '@mui/icons-material/ExpandMoreRounded'
import { api, formatSize, type DiskInfo, type DisksStatus, type PoolDevice } from '../api'

const day = (t: number) => new Date(t * 1000).toLocaleDateString('de-DE', { day: 'numeric', month: 'long', year: 'numeric' })

/** Wartet nach dem Einrichten, bis die Web-App auf dem neuen Speicher neu gestartet ist. */
async function waitForRestart() {
  // erst warten, bis sie weg ist, dann bis sie wieder da ist
  for (let i = 0; i < 20 && (await api.health().catch(() => false)); i++) await new Promise((r) => setTimeout(r, 1000))
  for (let i = 0; i < 120 && !(await api.health().catch(() => false)); i++) await new Promise((r) => setTimeout(r, 1000))
}

function profileText(profile: string, devices: number) {
  if (profile === 'single') return devices > 1 ? 'Ohne Spiegel' : 'Eine Platte, ohne Spiegel – fällt sie aus, sind die Daten weg'
  if (profile === 'raid1c3') return `3 Kopien über ${devices} Platten – zwei Platten dürfen ausfallen`
  return `Gespiegelt über ${devices} Platten – eine Platte darf ausfallen`
}

function SmartChip({ disk }: { disk?: DiskInfo }) {
  const s = disk?.smart
  if (!s) return null
  const bad = s.passed === false || (s.reallocated ?? 0) > 0 || (s.pending ?? 0) > 0
  return (
    <Chip
      size="small"
      color={bad ? 'error' : 'success'}
      variant="outlined"
      label={
        bad
          ? s.passed === false
            ? 'SMART: defekt'
            : `SMART: ${(s.reallocated ?? 0) + (s.pending ?? 0)} fehlerhafte Sektoren`
          : `Gesund${s.temperature ? ` · ${s.temperature} °C` : ''}${s.power_on_hours ? ` · ${Math.round(s.power_on_hours / 24)} Tage Betrieb` : ''}`
      }
    />
  )
}

function DiskLine({ disk, children, dim }: { disk: DiskInfo; children?: React.ReactNode; dim?: boolean }) {
  return (
    <Stack
      direction="row"
      sx={{ alignItems: 'center', gap: 1.5, p: 1.5, borderRadius: 2, border: 1, borderColor: 'divider', opacity: dim ? 0.6 : 1 }}
    >
      <SdStorageRoundedIcon color={disk.in_pool ? 'primary' : 'action'} />
      <Box sx={{ flex: 1, minWidth: 0 }}>
        <Typography sx={{ fontWeight: 600 }} noWrap>
          {disk.model} · {formatSize(disk.size)}
        </Typography>
        <Stack direction="row" sx={{ gap: 0.75, mt: 0.5, flexWrap: 'wrap', alignItems: 'center' }}>
          <Typography variant="body2" color="text.secondary" sx={{ fontFamily: 'ui-monospace, monospace' }}>
            {disk.name}
          </Typography>
          <Chip size="small" label={disk.ssd ? 'SSD' : 'HDD'} />
          {disk.transport && <Chip size="small" label={disk.transport.toUpperCase()} />}
          <SmartChip disk={disk} />
          {disk.old_member && <Chip size="small" color="info" variant="outlined" label="Frühere Platte dieses Speichers" />}
          {!disk.in_pool && disk.existing.length > 0 && !disk.old_member && (
            <Chip size="small" color="warning" variant="outlined" label={`enthält Daten (${disk.existing.join(', ')})`} />
          )}
        </Stack>
        {disk.reason && (
          <Typography variant="body2" color="text.secondary" sx={{ mt: 0.5 }}>
            <LockRoundedIcon sx={{ fontSize: 14, verticalAlign: 'text-bottom', mr: 0.5 }} />
            {disk.reason}
          </Typography>
        )}
      </Box>
      {children}
    </Stack>
  )
}

type Pending =
  | { kind: 'create'; devices: DiskInfo[]; profile: string }
  | { kind: 'add'; devices: DiskInfo[] }
  | { kind: 'replace'; devices: DiskInfo[]; devid: number }
  | { kind: 'remove'; devid: number; label: string }

/**
 * Festplatten: Speicherverbund (Spiegel) einrichten und verwalten.
 * `setupMode`: im Einrichtungsassistenten nur „einrichten“ anbieten.
 */
export default function DisksPanel({
  onToast,
  setupMode,
  onDone,
}: {
  onToast: (m: string) => void
  setupMode?: boolean
  onDone?: () => void
}) {
  const [st, setSt] = useState<DisksStatus | null>(null)
  const [selected, setSelected] = useState<string[]>([])
  const [profile, setProfile] = useState('raid1')
  const [pending, setPending] = useState<Pending | null>(null)
  const [restarting, setRestarting] = useState(false)
  const [showLocked, setShowLocked] = useState(false)
  const [error, setError] = useState('')
  const known = useRef<Set<string>>(new Set())

  const load = useCallback(() => {
    api
      .disks()
      .then((s) => {
        // Hot-Swap: neu eingesteckte Platten melden
        const now = new Set((s.disks ?? []).map((d) => d.path))
        if (known.current.size) {
          for (const d of s.disks ?? []) if (!known.current.has(d.path)) onToast(`Neue Platte erkannt: ${d.model} (${formatSize(d.size)})`)
        }
        known.current = now
        setSt(s)
      })
      .catch((e) => setError((e as Error).message))
  }, [onToast])

  useEffect(() => {
    load()
    const t = setInterval(load, 3000)
    return () => clearInterval(t)
  }, [load])

  if (restarting) {
    return (
      <Stack sx={{ alignItems: 'center', gap: 2, py: 6, textAlign: 'center' }}>
        <CircularProgress />
        <Typography sx={{ fontWeight: 600 }}>Speicher wird eingerichtet …</Typography>
        <Typography variant="body2" color="text.secondary" sx={{ maxWidth: 420 }}>
          {st?.job?.message ?? 'Vorhandene Daten werden kopiert, danach startet homevault auf dem neuen Speicher neu.'}
        </Typography>
      </Stack>
    )
  }
  if (!st) return <CircularProgress size={24} />
  if (!st.available) return <NotEnabled />
  if (st.reachable === false) return <Alert severity="error">Der Platten-Dienst ist nicht erreichbar: {st.error}</Alert>

  const disks = st.disks ?? []
  const pool = st.pool
  const usable = disks.filter((d) => d.usable)
  const locked = disks.filter((d) => !d.usable && !d.in_pool)
  const busy = !!st.job || pool?.scrub?.running || pool?.replace?.running || pool?.balance?.running

  async function run(p: Pending) {
    setError('')
    const confirm = 'devices' in p ? p.devices.map((d) => d.name) : []
    try {
      if (p.kind === 'create') {
        await api.diskAction('create', { devices: p.devices.map((d) => d.path), profile: p.profile, confirm })
        setPending(null)
        setRestarting(true)
        await waitForRestart()
        onToast('Speicher eingerichtet')
        if (onDone) onDone()
        else location.reload()
        return
      }
      if (p.kind === 'add') await api.diskAction('add', { device: p.devices[0].path, confirm })
      if (p.kind === 'replace') await api.diskAction('replace', { devid: p.devid, device: p.devices[0].path, confirm })
      if (p.kind === 'remove') await api.diskAction('remove', { devid: p.devid })
      onToast('Gestartet – der Fortschritt wird hier angezeigt')
      setPending(null)
      load()
    } catch (e) {
      setError((e as Error).message)
      setPending(null)
    }
  }

  const diskOf = (dev: PoolDevice) => disks.find((d) => d.path === dev.path)

  return (
    <Box>
      {error && (
        <Alert severity="error" sx={{ mb: 2 }} onClose={() => setError('')}>
          {error}
        </Alert>
      )}
      {st.last_error && !st.job && (
        <Alert severity="error" sx={{ mb: 2 }}>
          Letzter Vorgang fehlgeschlagen: {st.last_error}
        </Alert>
      )}
      {st.job && (
        <Alert severity="info" icon={<CircularProgress size={20} />} sx={{ mb: 2 }}>
          {st.job.message}
        </Alert>
      )}

      {pool ? (
        <>
          <PoolHeader st={st} />
          <Typography variant="subtitle2" sx={{ mt: 3, mb: 1 }}>
            Platten im Speicher
          </Typography>
          <Stack spacing={1}>
            {pool.devices.map((dev) =>
              dev.missing ? (
                <Stack
                  key={dev.devid}
                  direction="row"
                  sx={{ alignItems: 'center', gap: 1.5, p: 1.5, borderRadius: 2, border: 2, borderColor: 'error.main' }}
                >
                  <ErrorRoundedIcon color="error" />
                  <Box sx={{ flex: 1 }}>
                    <Typography sx={{ fontWeight: 600 }}>Platte fehlt oder ist defekt</Typography>
                    <Typography variant="body2" color="text.secondary">
                      {formatSize(dev.size ?? 0)} · Neue Platte einstecken und hier ersetzen
                    </Typography>
                  </Box>
                  <Button
                    variant="contained"
                    color="error"
                    startIcon={<SwapHorizRoundedIcon />}
                    disabled={busy || usable.length === 0}
                    onClick={() => setPending({ kind: 'replace', devices: [usable[0]], devid: dev.devid })}
                  >
                    Ersetzen
                  </Button>
                </Stack>
              ) : (
                <DiskLine
                  key={dev.devid}
                  disk={
                    diskOf(dev) ?? {
                      path: dev.path ?? '',
                      name: dev.path?.replace('/dev/', '') ?? '?',
                      size: dev.size ?? 0,
                      model: 'Platte',
                      ssd: false,
                      in_pool: true,
                      usable: false,
                      existing: [],
                    }
                  }
                >
                  <Stack sx={{ alignItems: 'flex-end', gap: 0.5 }}>
                    {(dev.errors ?? 0) > 0 && <Chip size="small" color="error" label={`${dev.errors} Fehler`} />}
                    {!setupMode && (
                      <Stack direction="row" sx={{ gap: 0.5 }}>
                        <Button
                          size="small"
                          startIcon={<SwapHorizRoundedIcon />}
                          disabled={busy || usable.length === 0}
                          onClick={() => setPending({ kind: 'replace', devices: [usable[0]], devid: dev.devid })}
                        >
                          Tauschen
                        </Button>
                        <Button
                          size="small"
                          color="inherit"
                          startIcon={<RemoveCircleOutlineRoundedIcon />}
                          disabled={busy}
                          onClick={() => setPending({ kind: 'remove', devid: dev.devid, label: diskOf(dev)?.model ?? dev.path ?? '' })}
                        >
                          Entfernen
                        </Button>
                      </Stack>
                    )}
                  </Stack>
                </DiskLine>
              ),
            )}
          </Stack>

          <Typography variant="subtitle2" sx={{ mt: 3, mb: 1 }}>
            Freie Platten
          </Typography>
          {usable.length === 0 ? (
            <Typography variant="body2" color="text.secondary">
              Keine freie Platte gefunden. Steck eine neue Platte ein (bei Wechselrahmen im laufenden Betrieb) – sie erscheint hier
              automatisch.
            </Typography>
          ) : (
            <Stack spacing={1}>
              {usable.map((d) => (
                <DiskLine key={d.path} disk={d}>
                  <Button startIcon={<AddRoundedIcon />} disabled={busy} onClick={() => setPending({ kind: 'add', devices: [d] })}>
                    Hinzufügen
                  </Button>
                </DiskLine>
              ))}
            </Stack>
          )}
        </>
      ) : (
        <CreatePool
          usable={usable}
          selected={selected}
          setSelected={setSelected}
          profile={profile}
          setProfile={setProfile}
          onCreate={() => setPending({ kind: 'create', devices: usable.filter((d) => selected.includes(d.path)), profile })}
        />
      )}

      {locked.length > 0 && (
        <Box sx={{ mt: 2 }}>
          <Button
            size="small"
            color="inherit"
            endIcon={<ExpandMoreRoundedIcon sx={{ transform: showLocked ? 'rotate(180deg)' : 'none' }} />}
            onClick={() => setShowLocked((v) => !v)}
          >
            {locked.length} gesperrte Laufwerke (System)
          </Button>
          <Collapse in={showLocked}>
            <Stack spacing={1} sx={{ mt: 1 }}>
              {locked.map((d) => (
                <DiskLine key={d.path} disk={d} dim />
              ))}
            </Stack>
          </Collapse>
        </Box>
      )}

      <ConfirmDialog pending={pending} usable={usable} onChange={setPending} onClose={() => setPending(null)} onConfirm={run} />
    </Box>
  )
}

function NotEnabled() {
  return (
    <Alert severity="info">
      <AlertTitle>Festplatten-Verwaltung ist nicht eingeschaltet</AlertTitle>
      Damit homevault Festplatten spiegeln und im laufenden Betrieb tauschen kann, in der <code>.env</code> auf dem Server die Zeilen unter
      „Festplatten-Verwaltung“ einkommentieren und <code>docker compose up -d</code> ausführen. Ohne sie speichert homevault einfach im
      Ordner <code>MEDIA_DIR</code>.
    </Alert>
  )
}

function PoolHeader({ st }: { st: DisksStatus }) {
  const pool = st.pool!
  const present = pool.devices.filter((d) => !d.missing).length
  const errors = pool.devices.reduce((a, d) => a + (d.errors ?? 0), 0)
  const smartBad = (st.disks ?? []).some(
    (d) => d.in_pool && d.smart && (d.smart.passed === false || (d.smart.reallocated ?? 0) + (d.smart.pending ?? 0) > 0),
  )
  const state = !pool.mounted
    ? {
        color: 'error.main',
        icon: <ErrorRoundedIcon color="error" sx={{ fontSize: 40 }} />,
        title: 'Speicher nicht eingehängt',
        text: 'Bitte die Platten prüfen.',
      }
    : pool.degraded
      ? {
          color: 'error.main',
          icon: <ErrorRoundedIcon color="error" sx={{ fontSize: 40 }} />,
          title: 'Notbetrieb – eine Platte fehlt',
          text: 'Deine Daten sind noch da, aber nicht mehr doppelt gesichert. Bitte die Platte so bald wie möglich ersetzen.',
        }
      : errors > 0 || smartBad
        ? {
            color: 'warning.main',
            icon: <WarningAmberRoundedIcon color="warning" sx={{ fontSize: 40 }} />,
            title: 'Warnung',
            text: 'Eine Platte meldet Fehler. Sie sollte bald getauscht werden.',
          }
        : {
            color: 'success.main',
            icon: <CheckCircleRoundedIcon color="success" sx={{ fontSize: 40 }} />,
            title: 'Alles in Ordnung',
            text: profileText(pool.data_profile ?? pool.profile, present),
          }
  const pct = pool.size ? ((pool.used ?? 0) / pool.size) * 100 : 0
  return (
    <Box sx={{ p: 2.5, borderRadius: 3, border: 2, borderColor: state.color, bgcolor: 'action.hover' }}>
      <Stack direction="row" sx={{ gap: 2, alignItems: 'center' }}>
        {state.icon}
        <Box sx={{ flex: 1 }}>
          <Typography variant="h6" sx={{ fontWeight: 700 }}>
            {state.title}
          </Typography>
          <Typography variant="body2" color="text.secondary">
            {state.text}
          </Typography>
        </Box>
      </Stack>
      {pool.size ? (
        <Box sx={{ mt: 2 }}>
          <LinearProgress variant="determinate" value={pct} sx={{ height: 8, borderRadius: 1 }} color={pct > 90 ? 'error' : 'primary'} />
          <Typography variant="body2" color="text.secondary" sx={{ mt: 0.5 }}>
            {formatSize(pool.used ?? 0)} von {formatSize(pool.size)} belegt · {formatSize(pool.free ?? 0)} frei (nutzbar nach Spiegelung)
          </Typography>
        </Box>
      ) : null}
      {(pool.replace?.running || pool.balance?.running || pool.scrub?.running) && (
        <Typography variant="body2" sx={{ mt: 1.5, display: 'flex', alignItems: 'center', gap: 1 }}>
          <CircularProgress size={14} />
          {pool.replace?.running
            ? `Daten werden auf die neue Platte kopiert … ${pool.replace.progress ?? 0} %`
            : pool.balance?.running
              ? 'Daten werden neu verteilt …'
              : 'Datenprüfung läuft …'}
        </Typography>
      )}
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, mt: 1.5, flexWrap: 'wrap' }}>
        <FactCheckRoundedIcon fontSize="small" color="action" />
        <Typography variant="body2" color="text.secondary" sx={{ flex: 1 }}>
          Datenprüfung (auf stille Fehler, monatlich automatisch):{' '}
          {pool.last_scrub
            ? `zuletzt am ${day(pool.last_scrub)}${pool.scrub?.errors ? ` – ${/no errors/i.test(pool.scrub.errors) ? 'keine Fehler gefunden' : pool.scrub.errors}` : ''}`
            : 'noch nie'}
        </Typography>
        <Button size="small" disabled={pool.scrub?.running || !!st.job} onClick={() => api.diskAction('scrub')}>
          Jetzt prüfen
        </Button>
      </Stack>
    </Box>
  )
}

function CreatePool({
  usable,
  selected,
  setSelected,
  profile,
  setProfile,
  onCreate,
}: {
  usable: DiskInfo[]
  selected: string[]
  setSelected: (s: string[]) => void
  profile: string
  setProfile: (p: string) => void
  onCreate: () => void
}) {
  const chosen = usable.filter((d) => selected.includes(d.path))
  const smallest = Math.min(...chosen.map((d) => d.size))
  const total = chosen.reduce((a, d) => a + d.size, 0)
  const effective = chosen.length < 2 ? 'single' : profile === 'raid1c3' && chosen.length < 3 ? 'raid1' : profile
  // Btrfs-RAID1: höchstens die Hälfte, und die größte Platte braucht ein Gegenstück
  const capacity =
    effective === 'single'
      ? total
      : effective === 'raid1c3'
        ? Math.min(total / 3, total - Math.max(...chosen.map((d) => d.size)))
        : Math.min(total / 2, total - Math.max(...chosen.map((d) => d.size)))

  return (
    <Box>
      <Stack direction="row" sx={{ gap: 1.5, alignItems: 'center', mb: 2 }}>
        <StorageRoundedIcon color="primary" />
        <Typography>
          Wähle die Platten aus, aus denen dein Speicher bestehen soll. Mit zwei oder mehr Platten werden alle Daten gespiegelt.
        </Typography>
      </Stack>
      {usable.length === 0 ? (
        <Alert severity="info">Keine freie Platte gefunden. Steck eine Platte ein – sie erscheint hier automatisch.</Alert>
      ) : (
        <Stack spacing={1}>
          {usable.map((d) => (
            <DiskLine key={d.path} disk={d}>
              <Checkbox
                checked={selected.includes(d.path)}
                onChange={(e) => setSelected(e.target.checked ? [...selected, d.path] : selected.filter((p) => p !== d.path))}
                slotProps={{ input: { 'aria-label': `${d.name} auswählen` } }}
              />
            </DiskLine>
          ))}
        </Stack>
      )}
      {chosen.length > 0 && (
        <Box sx={{ mt: 2, p: 2, borderRadius: 2, bgcolor: 'action.hover' }}>
          {chosen.length >= 3 && (
            <TextField
              select
              size="small"
              label="Schutz"
              value={profile}
              onChange={(e) => setProfile(e.target.value)}
              sx={{ mb: 1.5, minWidth: 280 }}
            >
              <MenuItem value="raid1">Spiegel – eine Platte darf ausfallen</MenuItem>
              <MenuItem value="raid1c3">3 Kopien – zwei Platten dürfen ausfallen</MenuItem>
            </TextField>
          )}
          {effective === 'single' ? (
            <Alert severity="warning" sx={{ mb: 1.5 }}>
              Mit nur einer Platte gibt es keinen Schutz bei einem Defekt. Du kannst später eine zweite hinzufügen, dann wird automatisch
              gespiegelt.
            </Alert>
          ) : (
            <Typography variant="body2" sx={{ mb: 1 }}>
              {profileText(effective, chosen.length)}.
            </Typography>
          )}
          <Typography variant="body2" color="text.secondary" sx={{ mb: 1.5 }}>
            Nutzbarer Platz: etwa <b>{formatSize(capacity)}</b>
            {effective !== 'single' &&
              chosen.length === 2 &&
              chosen[0].size !== chosen[1].size &&
              ` (die kleinere Platte mit ${formatSize(smallest)} bestimmt die Größe)`}
          </Typography>
          <Button variant="contained" startIcon={<StorageRoundedIcon />} onClick={onCreate}>
            Speicher einrichten
          </Button>
        </Box>
      )}
    </Box>
  )
}

function ConfirmDialog({
  pending,
  usable,
  onChange,
  onClose,
  onConfirm,
}: {
  pending: Pending | null
  usable: DiskInfo[]
  onChange: (p: Pending) => void
  onClose: () => void
  onConfirm: (p: Pending) => void
}) {
  const [typed, setTyped] = useState('')
  useEffect(() => setTyped(''), [pending])
  if (!pending) return null
  const devices = 'devices' in pending ? pending.devices : []
  const names = devices.map((d) => d.name).join(' ')
  const needsTyping = devices.length > 0
  const ok = !needsTyping || typed.trim() === names

  const title = {
    create: 'Speicher einrichten',
    add: 'Platte hinzufügen',
    replace: 'Platte tauschen',
    remove: 'Platte entfernen',
  }[pending.kind]

  return (
    <Dialog open onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>{title}</DialogTitle>
      <DialogContent>
        {pending.kind === 'replace' && usable.length > 1 && (
          <TextField
            select
            fullWidth
            size="small"
            label="Neue Platte"
            value={pending.devices[0].path}
            onChange={(e) => onChange({ ...pending, devices: [usable.find((d) => d.path === e.target.value)!] })}
            sx={{ mb: 2, mt: 1 }}
          >
            {usable.map((d) => (
              <MenuItem key={d.path} value={d.path}>
                {d.model} · {formatSize(d.size)} ({d.name})
              </MenuItem>
            ))}
          </TextField>
        )}
        {pending.kind === 'remove' ? (
          <DialogContentText>
            Die Daten von „{pending.label}“ werden auf die übrigen Platten verteilt, danach kannst du sie ausbauen. Das kann je nach
            Datenmenge Stunden dauern. Der Server bleibt dabei nutzbar.
          </DialogContentText>
        ) : (
          <>
            <Alert severity="warning" sx={{ mb: 2 }}>
              <AlertTitle>Alle Daten auf {devices.length === 1 ? 'dieser Platte' : 'diesen Platten'} werden gelöscht</AlertTitle>
              {devices.map((d) => (
                <Box key={d.path}>
                  {d.model} · {formatSize(d.size)} ({d.name}){d.existing.length > 0 && ` – enthält ${d.existing.join(', ')}`}
                </Box>
              ))}
            </Alert>
            {pending.kind === 'create' && (
              <DialogContentText sx={{ mb: 2 }}>
                Deine bisherigen Dateien, Benutzer und Alben werden auf den neuen Speicher kopiert. homevault startet danach einmal neu.
              </DialogContentText>
            )}
            {pending.kind === 'replace' && (
              <DialogContentText sx={{ mb: 2 }}>
                Die Daten werden auf die neue Platte kopiert. Danach kann die alte Platte ausgebaut werden. Der Server bleibt dabei nutzbar.
              </DialogContentText>
            )}
            <TextField
              fullWidth
              autoFocus
              label={`Zur Bestätigung „${names}“ eintippen`}
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
              autoComplete="off"
            />
          </>
        )}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Abbrechen</Button>
        <Button
          variant="contained"
          color={pending.kind === 'remove' ? 'inherit' : 'error'}
          disabled={!ok}
          onClick={() => onConfirm(pending)}
        >
          {title}
        </Button>
      </DialogActions>
    </Dialog>
  )
}
