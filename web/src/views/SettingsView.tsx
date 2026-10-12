import { useCallback, useEffect, useState, type ReactNode } from 'react'
import {
  Alert,
  ButtonBase,
  Tab as MuiTab,
  Tabs,
  Box,
  Button,
  Card,
  CardContent,
  Chip,
  FormControlLabel,
  IconButton,
  LinearProgress,
  MenuItem,
  Stack,
  Switch,
  TextField,
  Tooltip,
  Typography,
} from '@mui/material'
import GroupRoundedIcon from '@mui/icons-material/GroupRounded'
import LanRoundedIcon from '@mui/icons-material/LanRounded'
import BackupRoundedIcon from '@mui/icons-material/BackupRounded'
import SyncRoundedIcon from '@mui/icons-material/SyncRounded'
import ShareRoundedIcon from '@mui/icons-material/ShareRounded'
import TuneRoundedIcon from '@mui/icons-material/TuneRounded'
import SdStorageRoundedIcon from '@mui/icons-material/SdStorageRounded'
import PieChartRoundedIcon from '@mui/icons-material/PieChartRounded'
import MemoryRoundedIcon from '@mui/icons-material/MemoryRounded'
import ContentCopyRoundedIcon from '@mui/icons-material/ContentCopyRounded'
import DeleteOutlineRoundedIcon from '@mui/icons-material/DeleteOutlineRounded'
import LockRoundedIcon from '@mui/icons-material/LockRounded'
import BackupSection from '../components/BackupSection'
import SyncSection from '../components/SyncSection'
import DisksPanel from '../components/DisksPanel'
import { navigate } from '../router'
import { copyText, expiryText, fullUrl } from '../components/ShareDialog'
import { api, formatSize, type IndexStatus, type Settings, type ShareLink, type Storage } from '../api'
import { useShell } from '../shell'
import UsersPanel from '../components/UsersPanel'
import { ViewHeader } from './common'

/** Inhalt einer Einstellungsseite als Karte */
function Panel({ children, title }: { children: ReactNode; title?: string }) {
  return (
    <Card variant="outlined" sx={{ mb: 2, borderRadius: 3 }}>
      <CardContent sx={{ p: { xs: 2, sm: 3 } }}>
        {title && (
          <Typography variant="subtitle1" sx={{ fontWeight: 700, mb: 2 }}>
            {title}
          </Typography>
        )}
        {children}
      </CardContent>
    </Card>
  )
}

interface Tab {
  id: string
  label: string
  icon: ReactNode
  description: string
  admin?: boolean
  render: () => ReactNode
}

const TRANSCODE_SAVED = {
  now: 'Videos werden gleich nach dem Hochladen umgewandelt',
  night: 'Videos werden nachts zwischen 1 und 7 Uhr umgewandelt',
  demand: 'Videos werden erst beim Abspielen umgewandelt',
}

export default function SettingsView({ tab }: { tab?: string }) {
  const shell = useShell()
  const admin = shell.user.admin
  const tabs: Tab[] = [
    {
      id: 'allgemein',
      label: 'Allgemein',
      icon: <TuneRoundedIcon />,
      admin: true,
      description: 'Name des Servers, Papierkorb, Videos und Eingang',
      render: () => <ServerSection />,
    },
    {
      id: 'benutzer',
      label: 'Benutzer',
      icon: <GroupRoundedIcon />,
      admin: true,
      description: 'Wer Zugriff hat – die Logins gelten für Web und Netzlaufwerk',
      render: () => (
        <Panel>
          <UsersPanel me={shell.user} onToast={shell.toast} />
        </Panel>
      ),
    },
    {
      id: 'festplatten',
      label: 'Festplatten',
      icon: <SdStorageRoundedIcon />,
      admin: true,
      description: 'Spiegel (RAID), Zustand der Platten und Tausch im laufenden Betrieb',
      render: () => (
        <Panel>
          <DisksPanel onToast={shell.toast} />
        </Panel>
      ),
    },
    {
      id: 'speicher',
      label: 'Speicherplatz',
      icon: <PieChartRoundedIcon />,
      description: 'Wie viel Platz belegt und frei ist',
      render: () => <StorageSection />,
    },
    {
      id: 'backup',
      label: 'Backup',
      icon: <BackupRoundedIcon />,
      admin: true,
      description: 'Automatische Sicherung auf andere NAS im Haus',
      render: () => (
        <Panel>
          <BackupSection />
        </Panel>
      ),
    },
    {
      id: 'sync',
      label: 'Synchronisieren',
      icon: <SyncRoundedIcon />,
      description: 'Ordner laufend auf andere NAS im Haus spiegeln',
      render: () => (
        <Panel>
          <SyncSection />
        </Panel>
      ),
    },
    {
      id: 'teilen',
      label: 'Teilen-Links',
      icon: <ShareRoundedIcon />,
      description: 'Deine Links für Leute ohne Konto',
      render: () => (
        <Panel>
          <SharesPanel />
        </Panel>
      ),
    },
    {
      id: 'netzlaufwerk',
      label: 'Netzlaufwerk',
      icon: <LanRoundedIcon />,
      description: 'Vom Computer oder Handy aus verbinden',
      render: () => <SmbSection />,
    },
    {
      id: 'system',
      label: 'System',
      icon: <MemoryRoundedIcon />,
      admin: true,
      description: 'Medien-Index, Video-Umwandlung und Vorschau-Funktionen',
      render: () => <IndexSection />,
    },
  ].filter((t) => admin || !t.admin)
  const current = tabs.find((t) => t.id === tab) ?? tabs[0]
  const go = (id: string) => navigate({ view: 'settings', tab: id })

  return (
    <Box>
      <ViewHeader title="Einstellungen" />
      {/* Handy: Reiter zum Wischen */}
      <Tabs
        value={current.id}
        onChange={(_, v) => go(v)}
        variant="scrollable"
        scrollButtons={false}
        sx={{ display: { xs: 'flex', md: 'none' }, mb: 2, borderBottom: 1, borderColor: 'divider' }}
      >
        {tabs.map((t) => (
          <MuiTab key={t.id} value={t.id} label={t.label} />
        ))}
      </Tabs>
      <Stack direction="row" sx={{ gap: 3, alignItems: 'flex-start' }}>
        {/* Desktop: Navigation links */}
        <Box component="nav" sx={{ display: { xs: 'none', md: 'block' }, width: 220, flexShrink: 0, position: 'sticky', top: 88 }}>
          {tabs.map((t) => (
            <ButtonBase
              key={t.id}
              onClick={() => go(t.id)}
              sx={{
                width: '100%',
                justifyContent: 'flex-start',
                gap: 1.5,
                px: 2,
                py: 1.25,
                mb: 0.5,
                borderRadius: 2,
                color: t.id === current.id ? 'primary.main' : 'text.secondary',
                bgcolor: t.id === current.id ? 'action.selected' : 'transparent',
                fontWeight: t.id === current.id ? 700 : 500,
                '&:hover': { bgcolor: 'action.hover' },
              }}
            >
              {t.icon}
              <Typography sx={{ fontWeight: 'inherit', color: t.id === current.id ? 'text.primary' : 'inherit' }}>{t.label}</Typography>
            </ButtonBase>
          ))}
        </Box>
        <Box sx={{ flex: 1, minWidth: 0, maxWidth: 860 }}>
          <Stack direction="row" sx={{ alignItems: 'center', gap: 2, mb: 2.5 }}>
            <Box
              sx={{
                width: 48,
                height: 48,
                borderRadius: 3,
                display: 'grid',
                placeItems: 'center',
                bgcolor: 'action.selected',
                color: 'primary.main',
                flexShrink: 0,
              }}
            >
              {current.icon}
            </Box>
            <Box>
              <Typography variant="h6" sx={{ fontWeight: 700, lineHeight: 1.2 }}>
                {current.label}
              </Typography>
              <Typography variant="body2" color="text.secondary">
                {current.description}
              </Typography>
            </Box>
          </Stack>
          {current.render()}
        </Box>
      </Stack>
    </Box>
  )
}

// ---------- Speicher ----------

function Bar({ label, used, total, warn }: { label: string; used: number; total: number; warn?: boolean }) {
  const pct = total ? Math.min(100, (used / total) * 100) : 0
  return (
    <Box sx={{ mb: 2 }}>
      <Stack direction="row" sx={{ justifyContent: 'space-between', mb: 0.5 }}>
        <Typography variant="body2" sx={{ fontWeight: 500 }}>
          {label}
        </Typography>
        <Typography variant="body2" color="text.secondary">
          {formatSize(used)} von {formatSize(total)} ({Math.round(pct)} %)
        </Typography>
      </Stack>
      <LinearProgress
        variant="determinate"
        value={pct}
        color={warn || pct > 90 ? 'error' : 'primary'}
        sx={{ height: 8, borderRadius: 1 }}
      />
    </Box>
  )
}

function StorageSection() {
  const [s, setS] = useState<Storage | null>(null)
  const load = useCallback(() => {
    api
      .storage()
      .then(setS)
      .catch(() => {})
  }, [])
  useEffect(load, [load])

  if (!s) return null
  const { disk } = s
  return (
    <Panel>
      {s.warning && (
        <Alert severity="warning" sx={{ mb: 2 }}>
          Der Speicher wird knapp: nur noch {formatSize(disk.free)} frei. Leere den Papierkorb oder lösche große Videos.
        </Alert>
      )}
      {disk.total > 0 && <Bar label="Festplatte" used={disk.total - disk.free} total={disk.total} warn={s.warning} />}
      {s.me.quota != null ? (
        <Bar label="Dein privater Bereich (Limit)" used={s.me.used} total={s.me.quota} />
      ) : (
        <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
          Dein privater Bereich belegt {formatSize(s.me.used)} (kein Limit).
        </Typography>
      )}

      {s.users && (
        <>
          <Typography variant="subtitle2" sx={{ mb: 1 }}>
            Belegung
          </Typography>
          <Stack spacing={0.5} sx={{ mb: 1 }}>
            <UsageRow
              label="Gemeinsam"
              bytes={s.shared.bytes}
              detail={`${formatSize(s.shared.photo_bytes)} Fotos · ${formatSize(s.shared.video_bytes)} Videos`}
            />
            {s.users.map((u) => (
              <UsageRow
                key={u.name}
                label={`Privat: ${u.name}`}
                bytes={u.usage.bytes}
                detail={u.quota ? `Limit ${formatSize(u.quota)}` : 'kein Limit'}
              />
            ))}
            <UsageRow label="Papierkorb" bytes={s.trash ?? 0} />
            <UsageRow label="Vorschaubilder & Video-Cache" bytes={s.cache ?? 0} />
          </Stack>
          <Typography variant="caption" color="text.secondary">
            Stand: {s.last_scan ? new Date(s.last_scan * 1000).toLocaleTimeString('de-DE') : '–'} (wird beim Medien-Index mitgezählt)
          </Typography>
        </>
      )}
    </Panel>
  )
}

function UsageRow({ label, bytes, detail, action }: { label: string; bytes: number; detail?: string; action?: ReactNode }) {
  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 1, py: 0.5, borderBottom: 1, borderColor: 'divider' }}>
      <Typography variant="body2" sx={{ flex: 1 }}>
        {label}
        {detail && (
          <Typography component="span" variant="body2" color="text.secondary" sx={{ ml: 1 }}>
            {detail}
          </Typography>
        )}
      </Typography>
      <Typography variant="body2" sx={{ fontVariantNumeric: 'tabular-nums' }}>
        {formatSize(bytes)}
      </Typography>
      <Box sx={{ width: 34 }}>{action}</Box>
    </Stack>
  )
}

// ---------- Teilen-Links ----------

function SharesPanel() {
  const shell = useShell()
  const [links, setLinks] = useState<ShareLink[] | null>(null)
  const load = useCallback(() => {
    api
      .shares()
      .then((r) => setLinks(r.shares))
      .catch(() => setLinks([]))
  }, [])
  useEffect(load, [load])

  if (links === null) return null
  if (links.length === 0) {
    return (
      <Typography variant="body2" color="text.secondary">
        Du hast noch nichts geteilt. Öffne ein Album oder Foto und tippe auf „Teilen“, um einen Link für Leute ohne Konto zu erstellen.
      </Typography>
    )
  }
  return (
    <Stack spacing={1}>
      {links.map((l) => (
        <Stack key={l.id} direction="row" sx={{ alignItems: 'center', gap: 1, p: 1, borderRadius: 2, bgcolor: 'action.hover' }}>
          <Box sx={{ flex: 1, minWidth: 0 }}>
            <Typography noWrap sx={{ fontWeight: 500 }}>
              {l.kind === 'album' ? 'Album: ' : ''}
              {l.title ?? 'Gelöschte Datei'}
            </Typography>
            <Stack direction="row" sx={{ gap: 0.5, mt: 0.5, flexWrap: 'wrap' }}>
              {l.has_password && <Chip size="small" icon={<LockRoundedIcon />} label="Passwort" />}
              <Chip size="small" label={expiryText(l)} color={l.expired ? 'warning' : 'default'} />
              <Chip size="small" label={`${l.views}× geöffnet`} />
            </Stack>
          </Box>
          <Tooltip title="Link kopieren">
            <IconButton onClick={() => copyText(fullUrl(l.url)).then(() => shell.toast('Link kopiert'))}>
              <ContentCopyRoundedIcon />
            </IconButton>
          </Tooltip>
          <Tooltip title="Link löschen">
            <IconButton
              onClick={() =>
                api
                  .deleteShare(l.id)
                  .then(() => {
                    shell.toast('Link gelöscht')
                    load()
                  })
                  .catch((e) => shell.toast((e as Error).message))
              }
            >
              <DeleteOutlineRoundedIcon />
            </IconButton>
          </Tooltip>
        </Stack>
      ))}
    </Stack>
  )
}

// ---------- Server (Admin) ----------

function ServerSection() {
  const shell = useShell()
  const [settings, setSettings] = useState<Settings | null>(null)
  const [name, setName] = useState('')

  useEffect(() => {
    api
      .settings()
      .then((s) => {
        setSettings(s)
        setName(s.server_name)
      })
      .catch(() => {})
  }, [])

  async function save(patch: Partial<Settings>, msg = 'Gespeichert') {
    try {
      await api.updateSettings(patch)
      setSettings((s) => (s ? { ...s, ...patch } : s))
      if (patch.server_name) shell.refreshMe()
      shell.toast(msg)
    } catch (e) {
      shell.toast((e as Error).message)
    }
  }

  if (!settings) return null
  return (
    <Panel>
      <Stack spacing={2.5}>
        <Stack
          component="form"
          direction="row"
          spacing={1.5}
          onSubmit={(e) => {
            e.preventDefault()
            save({ server_name: name.trim() })
          }}
        >
          <TextField size="small" label="Servername" value={name} onChange={(e) => setName(e.target.value)} sx={{ flex: 1 }} />
          <Button type="submit" variant="contained" disabled={!name.trim() || name.trim() === settings.server_name}>
            Speichern
          </Button>
        </Stack>
        <TextField
          select
          size="small"
          label="Papierkorb automatisch leeren nach"
          value={settings.trash_days}
          onChange={(e) => save({ trash_days: Number(e.target.value) })}
        >
          {[7, 14, 30, 60, 90, 365].map((d) => (
            <MenuItem key={d} value={d}>
              {d === 365 ? '1 Jahr' : `${d} Tagen`}
            </MenuItem>
          ))}
        </TextField>
        <TextField
          select
          label="iPhone-Videos (HEVC) für alle Browser umwandeln"
          value={settings.transcode_mode}
          onChange={(e) =>
            save(
              { transcode_mode: e.target.value as Settings['transcode_mode'] },
              TRANSCODE_SAVED[e.target.value as Settings['transcode_mode']],
            )
          }
          helperText="Beim Umwandeln rechnet der Server viel (und der Lüfter wird lauter). Nachts merkt das niemand; „beim Abspielen“ spart am meisten, dafür wartet man beim ersten Mal."
        >
          <MenuItem value="now">Sofort nach dem Hochladen</MenuItem>
          <MenuItem value="night">Nur nachts (1–7 Uhr)</MenuItem>
          <MenuItem value="demand">Erst beim Abspielen</MenuItem>
        </TextField>
        <Box>
          <FormControlLabel
            control={
              <Switch
                checked={settings.inbox_enabled}
                onChange={(e) =>
                  save(
                    { inbox_enabled: e.target.checked },
                    e.target.checked ? 'Eingang wird automatisch sortiert' : 'Eingang wird nicht mehr sortiert',
                  )
                }
              />
            }
            label="Eingangs-Ordner automatisch sortieren"
          />
          <Typography variant="body2" color="text.secondary" sx={{ ml: 6 }}>
            Fotos und Dokumente im Ordner „Eingang“ (Gemeinsam und Privat) werden nach Datum bzw. per KI benannt in Ordner einsortiert.
          </Typography>
        </Box>
      </Stack>
    </Panel>
  )
}

// ---------- Medien-Index (Admin) ----------

function IndexSection() {
  const shell = useShell()
  const [status, setStatus] = useState<IndexStatus | null>(null)
  const [dbSize, setDbSize] = useState(0)
  const [transcoding, setTranscoding] = useState<{ current: { name: string; progress: number } | null; queued: number } | null>(null)

  const load = useCallback(() => {
    api
      .indexStatus()
      .then((r) => {
        setStatus(r.index)
        setDbSize(r.db_size)
        setTranscoding(r.transcoder)
      })
      .catch(() => {})
  }, [])

  useEffect(() => {
    load()
    const t = setInterval(load, 3000)
    return () => clearInterval(t)
  }, [load])

  return (
    <Panel>
      <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
        Der Index erfasst alle Fotos und Videos mit Aufnahmedatum für Zeitleiste, Favoriten und Alben. Er aktualisiert sich nach Uploads
        sofort und prüft alle 10 Minuten auf Änderungen über das Netzlaufwerk.
      </Typography>
      {status?.scanning && <LinearProgress sx={{ mb: 2, borderRadius: 1 }} />}
      {status?.last_error && (
        <Alert severity="error" sx={{ mb: 2 }}>
          {status.last_error}
        </Alert>
      )}
      <Stack direction={{ xs: 'column', sm: 'row' }} spacing={{ xs: 1, sm: 4 }} sx={{ mb: 2 }}>
        <Stat label="Erfasst" value={status ? `${status.items.toLocaleString('de-DE')} Fotos & Videos` : '–'} />
        <Stat
          label="Letzter Durchlauf"
          value={
            status?.last_scan
              ? `${new Date(status.last_scan * 1000).toLocaleTimeString('de-DE')} (${status.last_duration_ms} ms)`
              : status?.scanning
                ? 'läuft …'
                : '–'
          }
        />
        <Stat label="Datenbank" value={formatSize(dbSize)} />
      </Stack>
      {transcoding?.current && (
        <Box sx={{ mb: 2 }}>
          <Typography variant="body2" sx={{ mb: 0.5 }}>
            Wandle Video um: {transcoding.current.name} ({Math.round(transcoding.current.progress * 100)} %)
            {transcoding.queued > 0 && `, ${transcoding.queued} angefragt`}
          </Typography>
          <LinearProgress variant="determinate" value={transcoding.current.progress * 100} sx={{ borderRadius: 1 }} />
        </Box>
      )}
      <Button
        variant="outlined"
        disabled={status?.scanning}
        onClick={() =>
          api
            .rescan()
            .then(() => {
              shell.toast('Index wird neu eingelesen')
              setTimeout(load, 500)
            })
            .catch((e) => shell.toast((e as Error).message))
        }
      >
        Jetzt neu einlesen
      </Button>
    </Panel>
  )
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <Box>
      <Typography variant="caption" color="text.secondary">
        {label}
      </Typography>
      <Typography sx={{ fontWeight: 500 }}>{value}</Typography>
    </Box>
  )
}

// ---------- Netzlaufwerk ----------

export function SmbSection() {
  const host = location.hostname
  return (
    <Panel>
      <SmbInfo host={host} />
    </Panel>
  )
}

/** Anleitung zum Verbinden, wird auch im Einrichtungsassistenten gezeigt. */
export function SmbInfo({ host }: { host: string }) {
  const code = { fontFamily: 'ui-monospace, monospace', bgcolor: 'action.hover', px: 0.75, py: 0.25, borderRadius: 1 }
  return (
    <Stack spacing={1.5}>
      <Typography variant="body2" color="text.secondary">
        Alle Benutzer melden sich mit denselben Zugangsdaten wie hier an. Es gibt zwei Freigaben: <b>Gemeinsam</b> und <b>Privat</b> (jeder
        sieht dort nur seinen eigenen Ordner). Über das Netzlaufwerk Gelöschtes landet nicht im Papierkorb.
      </Typography>
      <Box>
        <Typography variant="subtitle2">Windows (Explorer)</Typography>
        <Box component="code" sx={code}>{`\\\\${host}\\Gemeinsam`}</Box>
      </Box>
      <Box>
        <Typography variant="subtitle2">macOS (Finder → Gehe zu → Mit Server verbinden)</Typography>
        <Box component="code" sx={code}>{`smb://${host}`}</Box>
      </Box>
      <Box>
        <Typography variant="subtitle2">iPhone/iPad (Dateien → … → Mit Server verbinden)</Typography>
        <Box component="code" sx={code}>{`smb://${host}`}</Box>
      </Box>
    </Stack>
  )
}
