import { useEffect, useState } from 'react'
import {
  Alert,
  Box,
  Button,
  CircularProgress,
  LinearProgress,
  Paper,
  Stack,
  Step,
  StepLabel,
  Stepper,
  TextField,
  Typography,
} from '@mui/material'
import PhotoLibraryRoundedIcon from '@mui/icons-material/PhotoLibraryRounded'
import PersonRoundedIcon from '@mui/icons-material/PersonRounded'
import GroupsRoundedIcon from '@mui/icons-material/GroupsRounded'
import LanRoundedIcon from '@mui/icons-material/LanRounded'
import CheckCircleRoundedIcon from '@mui/icons-material/CheckCircleRounded'
import RestoreRoundedIcon from '@mui/icons-material/RestoreRounded'
import { api, formatSize, type BackupProgress, type User } from '../api'
import UsersPanel from '../components/UsersPanel'
import DisksPanel from '../components/DisksPanel'
import { SmbInfo } from '../views/SettingsView'

type StepId = 'welcome' | 'admin' | 'disks' | 'users' | 'done' | 'restore'
const LABELS: Record<StepId, string> = {
  welcome: 'Willkommen',
  admin: 'Admin-Konto',
  disks: 'Festplatten',
  users: 'Benutzer',
  done: 'Fertig',
  restore: 'Wiederherstellen',
}

/**
 * Einrichtungsassistent beim allerersten Start: Servername und Admin-Konto,
 * Festplatten spiegeln (falls die Verwaltung eingeschaltet ist), weitere
 * Benutzer, danach Hinweise zum Netzlaufwerk.
 */
export default function Setup({ onDone }: { onDone: () => void }) {
  const [step, setStep] = useState<StepId>('welcome')
  const [admin, setAdmin] = useState<User | null>(null)
  const [withDisks, setWithDisks] = useState(false)
  const [message, setMessage] = useState('')
  const steps: StepId[] =
    step === 'restore' ? ['welcome', 'restore'] : ['welcome', 'admin', ...(withDisks ? (['disks'] as const) : []), 'users', 'done']

  return (
    <Box sx={{ minHeight: '100dvh', display: 'grid', placeItems: 'center', p: { xs: 1.5, sm: 3 } }}>
      <Paper variant="outlined" sx={{ width: '100%', maxWidth: 680, p: { xs: 2.5, sm: 4 }, borderRadius: 4 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5, mb: 3 }}>
          <PhotoLibraryRoundedIcon color="primary" sx={{ fontSize: 36 }} />
          <Typography variant="h5" sx={{ fontWeight: 700 }}>
            homevault einrichten
          </Typography>
        </Stack>
        <Stepper activeStep={steps.indexOf(step)} alternativeLabel sx={{ mb: 4 }}>
          {steps.map((s) => (
            <Step key={s}>
              <StepLabel>{LABELS[s]}</StepLabel>
            </Step>
          ))}
        </Stepper>

        {step === 'welcome' && <Welcome onNext={() => setStep('admin')} onRestore={() => setStep('restore')} />}
        {step === 'restore' && <RestoreSetup onBack={() => setStep('welcome')} />}
        {step === 'admin' && (
          <AdminStep
            onCreated={async (u) => {
              setAdmin(u)
              // Festplatten-Schritt nur, wenn die Verwaltung eingeschaltet und noch nichts eingerichtet ist
              const d = await api.disks().catch(() => null)
              const disks = !!d?.available && d.reachable !== false && !d.pool
              setWithDisks(disks)
              setStep(disks ? 'disks' : 'users')
            }}
          />
        )}
        {step === 'disks' && (
          <Box>
            <Typography sx={{ mb: 2 }} color="text.secondary">
              Damit ein Festplatten-Defekt keine Fotos kostet, werden alle Daten auf zwei oder mehr Platten gespiegelt. Du kannst das auch
              später in den Einstellungen einrichten.
            </Typography>
            <DisksPanel setupMode onToast={setMessage} onDone={() => setStep('users')} />
            <Stack direction="row" sx={{ justifyContent: 'flex-end', mt: 3 }}>
              <Button onClick={() => setStep('users')}>Überspringen</Button>
            </Stack>
          </Box>
        )}
        {step === 'users' && admin && (
          <Box>
            <Typography sx={{ mb: 2 }} color="text.secondary">
              Lege jetzt die Konten für deine Familie oder Mitbewohner an. Das geht auch später in den Einstellungen.
            </Typography>
            {message && (
              <Alert severity="success" sx={{ mb: 2 }} onClose={() => setMessage('')}>
                {message}
              </Alert>
            )}
            <UsersPanel me={admin} onToast={setMessage} />
            <Stack direction="row" sx={{ justifyContent: 'flex-end', mt: 3 }}>
              <Button variant="contained" onClick={() => setStep('done')}>
                Weiter
              </Button>
            </Stack>
          </Box>
        )}
        {step === 'done' && (
          <Box>
            <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5, mb: 2 }}>
              <CheckCircleRoundedIcon color="success" sx={{ fontSize: 32 }} />
              <Typography variant="h6">Alles bereit!</Typography>
            </Stack>
            <Typography color="text.secondary" sx={{ mb: 3 }}>
              Du kannst jetzt Fotos hochladen. Außerdem ist dein Server als Netzlaufwerk erreichbar:
            </Typography>
            <SmbInfo host={location.hostname} />
            <Stack direction="row" sx={{ justifyContent: 'flex-end', mt: 4 }}>
              <Button variant="contained" size="large" onClick={onDone}>
                Los geht’s
              </Button>
            </Stack>
          </Box>
        )}
      </Paper>
    </Box>
  )
}

function Welcome({ onNext, onRestore }: { onNext: () => void; onRestore: () => void }) {
  const point = (icon: React.ReactNode, title: string, text: string) => (
    <Stack direction="row" sx={{ gap: 2 }}>
      <Box sx={{ color: 'primary.main', pt: 0.25 }}>{icon}</Box>
      <Box>
        <Typography sx={{ fontWeight: 600 }}>{title}</Typography>
        <Typography variant="body2" color="text.secondary">
          {text}
        </Typography>
      </Box>
    </Stack>
  )
  return (
    <Stack spacing={2.5}>
      <Typography>Willkommen! In wenigen Schritten ist dein Server bereit.</Typography>
      {point(<PersonRoundedIcon />, 'Privat', 'Jeder Benutzer bekommt einen eigenen Bereich, den nur er sieht.')}
      {point(<GroupsRoundedIcon />, 'Gemeinsam', 'Ein Bereich für alle, z. B. für Familienfotos.')}
      {point(
        <LanRoundedIcon />,
        'Netzlaufwerk',
        'Beide Bereiche sind auch im Windows-Explorer, Finder und in der iPhone-Dateien-App erreichbar.',
      )}
      <Stack direction="row" sx={{ justifyContent: 'flex-end', pt: 1 }}>
        <Button onClick={onRestore} startIcon={<RestoreRoundedIcon />}>
          Aus Backup wiederherstellen
        </Button>
        <Button variant="contained" onClick={onNext}>
          Einrichtung starten
        </Button>
      </Stack>
    </Stack>
  )
}

function AdminStep({ onCreated }: { onCreated: (u: User) => void | Promise<void> }) {
  const [serverName, setServerName] = useState('homevault')
  const [name, setName] = useState('')
  const [password, setPassword] = useState('')
  const [repeat, setRepeat] = useState('')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const mismatch = repeat.length > 0 && password !== repeat

  async function submit() {
    setBusy(true)
    setError('')
    try {
      const n = name.trim().toLowerCase()
      await api.setup({ server_name: serverName.trim(), name: n, password })
      await onCreated({ name: n, admin: true })
    } catch (e) {
      setError((e as Error).message)
      setBusy(false)
    }
  }

  return (
    <Stack
      component="form"
      spacing={2}
      onSubmit={(e) => {
        e.preventDefault()
        submit()
      }}
    >
      {error && <Alert severity="error">{error}</Alert>}
      <TextField
        label="Name des Servers"
        value={serverName}
        onChange={(e) => setServerName(e.target.value)}
        helperText="Wird oben in der App angezeigt"
      />
      <Typography variant="subtitle2" sx={{ pt: 1 }}>
        Dein Administrator-Konto
      </Typography>
      <TextField
        label="Benutzername"
        autoFocus
        autoComplete="username"
        autoCapitalize="none"
        value={name}
        onChange={(e) => setName(e.target.value.toLowerCase())}
        helperText="Kleinbuchstaben, Ziffern, _ und -"
      />
      <TextField
        label="Passwort"
        type="password"
        autoComplete="new-password"
        value={password}
        onChange={(e) => setPassword(e.target.value)}
      />
      <TextField
        label="Passwort wiederholen"
        type="password"
        autoComplete="new-password"
        value={repeat}
        onChange={(e) => setRepeat(e.target.value)}
        error={mismatch}
        helperText={mismatch ? 'Passwörter stimmen nicht überein' : 'Gilt auch für das Netzlaufwerk'}
      />
      <Stack direction="row" sx={{ justifyContent: 'flex-end' }}>
        <Button type="submit" variant="contained" disabled={busy || !name.trim() || !password || password !== repeat}>
          Konto anlegen
        </Button>
      </Stack>
    </Stack>
  )
}

/**
 * Komplett-Wiederherstellung auf einem neuen Server: Benutzer, Alben,
 * Einstellungen und alle Dateien vom Backup-NAS laden.
 */
function RestoreSetup({ onBack }: { onBack: () => void }) {
  const [v, setV] = useState({ host: '', port: 445, share: '', folder: 'homevault', username: '', password: '' })
  const [error, setError] = useState('')
  const [running, setRunning] = useState<BackupProgress | null>(null)
  const [started, setStarted] = useState(false)
  const set = (k: keyof typeof v, value: string | number) => setV((p) => ({ ...p, [k]: value }))

  async function start() {
    setError('')
    try {
      await api.setupRestore(v)
      setStarted(true)
    } catch (e) {
      setError((e as Error).message)
    }
  }

  useEffect(() => {
    if (!started) return
    let stop = false
    const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))
    const poll = async () => {
      // Fortschritt abfragen; Fehler beim Abfragen sind meist nur kurze Aussetzer
      while (!stop) {
        const s = await api.setupRestoreStatus().catch(() => null)
        if (stop) return
        // done: Datenbank ist übernommen, der Server läuft schon neu
        if (s?.done) {
          location.reload()
          return
        }
        if (s?.result && !s.result.ok) {
          setError(s.result.message ?? 'Fehler')
          setStarted(false)
          return
        }
        if (s?.result?.ok) break
        if (s) setRunning(s.running)
        await sleep(1500)
      }
      // Server startet neu und übernimmt die Datenbank: warten, bis er wieder antwortet
      while (!stop) {
        await sleep(2000)
        const me = await api.me().catch(() => null)
        if (stop) return
        if (me && !me.setup_needed) {
          location.reload()
          return
        }
      }
    }
    poll()
    return () => {
      stop = true
    }
  }, [started])

  if (started) {
    const pct = running?.total_bytes ? (running.bytes / running.total_bytes) * 100 : undefined
    return (
      <Stack sx={{ alignItems: 'center', gap: 2, py: 4, textAlign: 'center' }}>
        <CircularProgress />
        <Typography sx={{ fontWeight: 600 }}>Daten werden vom Backup geladen …</Typography>
        <Typography variant="body2" color="text.secondary">
          {running?.part || 'Startet'}
          {running?.total_transfers ? ` · ${running.transfers} von ${running.total_transfers} Dateien` : ''}
          {running?.total_bytes ? ` · ${formatSize(running.bytes)} von ${formatSize(running.total_bytes)}` : ''}
        </Typography>
        <LinearProgress variant={pct === undefined ? 'indeterminate' : 'determinate'} value={pct} sx={{ width: '80%', borderRadius: 1 }} />
        <Typography variant="body2" color="text.secondary">
          Danach startet homevault neu. Melde dich dann mit deinem bisherigen Benutzer an.
        </Typography>
      </Stack>
    )
  }

  return (
    <Stack
      component="form"
      spacing={2}
      onSubmit={(e) => {
        e.preventDefault()
        start()
      }}
    >
      <Typography color="text.secondary">
        Gib an, wo dein Backup liegt (z. B. das NAS deiner Eltern). homevault lädt Benutzer, Alben, Favoriten, Einstellungen und alle
        Dateien zurück.
      </Typography>
      {error && <Alert severity="error">{error}</Alert>}
      <Stack direction={{ xs: 'column', sm: 'row' }} spacing={1.5}>
        <TextField
          label="Server"
          placeholder="192.168.1.20"
          value={v.host}
          onChange={(e) => set('host', e.target.value)}
          sx={{ flex: 2 }}
          autoFocus
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
        <TextField label="Freigabe" placeholder="backup" value={v.share} onChange={(e) => set('share', e.target.value)} sx={{ flex: 1 }} />
        <TextField label="Ordner auf der Freigabe" value={v.folder} onChange={(e) => set('folder', e.target.value)} sx={{ flex: 1 }} />
      </Stack>
      <Stack direction={{ xs: 'column', sm: 'row' }} spacing={1.5}>
        <TextField
          label="Benutzer auf dem Backup-NAS"
          value={v.username}
          onChange={(e) => set('username', e.target.value)}
          sx={{ flex: 1 }}
        />
        <TextField label="Passwort" type="password" value={v.password} onChange={(e) => set('password', e.target.value)} sx={{ flex: 1 }} />
      </Stack>
      <Stack direction="row" sx={{ justifyContent: 'space-between' }}>
        <Button onClick={onBack}>Zurück</Button>
        <Button
          type="submit"
          variant="contained"
          startIcon={<RestoreRoundedIcon />}
          disabled={!v.host || !v.share || !v.username || !v.password}
        >
          Wiederherstellen
        </Button>
      </Stack>
    </Stack>
  )
}
