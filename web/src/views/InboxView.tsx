import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import {
  Alert,
  Avatar,
  Box,
  Button,
  Chip,
  CircularProgress,
  LinearProgress,
  Link,
  List,
  ListItem,
  ListItemAvatar,
  ListItemText,
  Stack,
  ToggleButton,
  ToggleButtonGroup,
  Typography,
} from '@mui/material'
import MoveToInboxRoundedIcon from '@mui/icons-material/MoveToInboxRounded'
import CloudUploadRoundedIcon from '@mui/icons-material/CloudUploadRounded'
import GroupsRoundedIcon from '@mui/icons-material/GroupsRounded'
import PersonRoundedIcon from '@mui/icons-material/PersonRounded'
import ImageOutlinedIcon from '@mui/icons-material/ImageOutlined'
import DescriptionOutlinedIcon from '@mui/icons-material/DescriptionOutlined'
import InsertDriveFileOutlinedIcon from '@mui/icons-material/InsertDriveFileOutlined'
import AutoAwesomeRoundedIcon from '@mui/icons-material/AutoAwesomeRounded'
import ArrowForwardRoundedIcon from '@mui/icons-material/ArrowForwardRounded'
import UndoRoundedIcon from '@mui/icons-material/UndoRounded'
import { api, parentPath, type InboxItem, type InboxStatus } from '../api'
import { hrefFor, navigate } from '../router'
import { useShell } from '../shell'
import { Empty, Loading, ViewHeader } from './common'

type Space = 'Gemeinsam' | 'Privat'

const inboxOf = (space: Space) => `${space}/Eingang`
/** 'Privat/anna' → 'Privat' */
const spaceOf = (area: string) => (area.startsWith('Privat') ? 'Privat' : 'Gemeinsam')

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

function KindIcon({ kind }: { kind: 'photo' | 'document' | 'other' }) {
  if (kind === 'photo') return <ImageOutlinedIcon />
  if (kind === 'document') return <DescriptionOutlinedIcon />
  return <InsertDriveFileOutlinedIcon />
}

// Fotos und Videos (werden ohne KI nach Datum/Ort sortiert)
const MEDIA = /\.(jpe?g|png|webp|gif|bmp|tiff?|hei[cf]|hif|mp4|m4v|mov|webm|mkv|avi|3gp)$/i

export default function InboxView() {
  const shell = useShell()
  const [data, setData] = useState<InboxStatus | null>(null)
  const [error, setError] = useState('')
  const [target, setTarget] = useState<Space>('Privat')
  const [dragging, setDragging] = useState(false)
  const [undoing, setUndoing] = useState<number | null>(null)
  const input = useRef<HTMLInputElement>(null)
  const dragDepth = useRef(0)

  const load = useCallback(() => {
    api
      .inbox()
      .then((d) => {
        setData(d)
        setError('')
      })
      .catch((e) => setError((e as Error).message))
  }, [])

  // alle 4 s aktualisieren, solange die Seite sichtbar ist
  useEffect(() => {
    load()
    const t = setInterval(() => !document.hidden && load(), 4000)
    const onVisible = () => !document.hidden && load()
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      clearInterval(t)
      document.removeEventListener('visibilitychange', onVisible)
    }
  }, [load])

  // fertige Uploads sofort als „wartet“ zeigen
  useEffect(() => shell.onUploaded((dir) => dir.endsWith('/Eingang') && load()), [shell, load])

  function upload(files: FileList | null) {
    if (files?.length) shell.upload(inboxOf(target), Array.from(files))
  }

  async function undo(item: InboxItem) {
    setUndoing(item.id)
    try {
      await api.inboxUndo(item.id)
      shell.toast(`„${item.original_name}“ zurückgelegt nach Eingang/Zurückgelegt`)
    } catch (e) {
      shell.toast((e as Error).message)
    }
    setUndoing(null)
    load()
  }

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
      upload(e.dataTransfer.files)
    },
  }

  const aiReady = data?.ai?.status.state === 'ready'

  return (
    <Box sx={{ maxWidth: 960 }}>
      <ViewHeader
        title="Eingang"
        subtitle={
          <>
            Lege Fotos und Dokumente in einen Eingang – sie werden automatisch einsortiert: Fotos nach Datum und Ort in „Fotos“, Dokumente
            bekommen von der KI einen Namen und landen in „Dokumente“. Alles lässt sich rückgängig machen, Unterordner bleiben unberührt.
          </>
        }
      />
      <Stack direction="row" sx={{ gap: 1, mb: 2, flexWrap: 'wrap' }}>
        <Button variant="outlined" startIcon={<GroupsRoundedIcon />} href={hrefFor({ view: 'files', path: inboxOf('Gemeinsam') })}>
          Gemeinsamer Eingang
        </Button>
        <Button variant="outlined" startIcon={<PersonRoundedIcon />} href={hrefFor({ view: 'files', path: inboxOf('Privat') })}>
          Mein Eingang
        </Button>
      </Stack>

      {/* Ablage: hochgeladene Dateien gehen direkt in den gewählten Eingang */}
      <Box
        {...dropHandlers}
        sx={{
          border: 2,
          borderStyle: 'dashed',
          borderColor: dragging ? 'primary.main' : 'divider',
          bgcolor: dragging ? 'action.selected' : 'transparent',
          borderRadius: 3,
          p: { xs: 2, sm: 3 },
          mb: 2,
          transition: 'background-color .15s, border-color .15s',
        }}
      >
        <Stack direction={{ xs: 'column', sm: 'row' }} sx={{ alignItems: { sm: 'center' }, gap: 2 }}>
          <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5, flex: 1, minWidth: 0 }}>
            <CloudUploadRoundedIcon color="primary" sx={{ fontSize: 40 }} />
            <Box>
              <Typography sx={{ fontWeight: 600 }}>{dragging ? 'Loslassen zum Hochladen' : 'Dateien hierher ziehen'}</Typography>
              <Typography variant="body2" color="text.secondary">
                Landen in „{inboxOf(target)}“ und werden dann einsortiert.
              </Typography>
            </Box>
          </Stack>
          <Stack direction="row" sx={{ alignItems: 'center', gap: 1, flexWrap: 'wrap' }}>
            <ToggleButtonGroup size="small" exclusive value={target} onChange={(_, v: Space | null) => v && setTarget(v)}>
              <ToggleButton value="Privat">Privat</ToggleButton>
              <ToggleButton value="Gemeinsam">Gemeinsam</ToggleButton>
            </ToggleButtonGroup>
            <Button variant="contained" onClick={() => input.current?.click()}>
              Auswählen
            </Button>
          </Stack>
        </Stack>
        <input
          ref={input}
          type="file"
          multiple
          hidden
          onChange={(e) => {
            upload(e.target.files)
            e.target.value = ''
          }}
        />
      </Box>

      {error && (
        <Alert severity="error" sx={{ mb: 2 }}>
          {error}
        </Alert>
      )}
      {data === null ? (
        !error && <Loading />
      ) : (
        <>
          <StatusLine data={data} admin={shell.user.admin} />

          {data.waiting.length > 0 && (
            <Section title={`Wartet (${data.waiting.length})`}>
              <List dense>
                {data.waiting.map((w) => {
                  const busy = data.current === `${w.area}/Eingang/${w.name}`
                  const state = busy
                    ? 'wird einsortiert …'
                    : !data.enabled
                      ? 'Sortieren ausgeschaltet'
                      : w.document && !aiReady
                        ? 'wartet auf KI'
                        : 'wartet'
                  return (
                    <ListItem key={`${w.area}/${w.name}`} disableGutters>
                      <ListItemAvatar>
                        <Avatar variant="rounded" sx={{ bgcolor: 'action.hover', color: 'text.secondary' }}>
                          {busy ? (
                            <CircularProgress size={22} />
                          ) : (
                            <KindIcon kind={w.document ? 'document' : MEDIA.test(w.name) ? 'photo' : 'other'} />
                          )}
                        </Avatar>
                      </ListItemAvatar>
                      <ListItemText
                        primary={w.name}
                        secondary={`${spaceOf(w.area)} · ${state}`}
                        slotProps={{ primary: { noWrap: true }, secondary: { noWrap: true } }}
                      />
                    </ListItem>
                  )
                })}
              </List>
            </Section>
          )}

          {data.items.length > 0 ? (
            <Section title="Zuletzt einsortiert">
              <List>
                {data.items.map((it) => (
                  <ItemRow key={it.id} item={it} undoing={undoing === it.id} onUndo={() => undo(it)} />
                ))}
              </List>
            </Section>
          ) : (
            data.waiting.length === 0 && (
              <Empty icon={<MoveToInboxRoundedIcon />}>
                Noch nichts einsortiert. Lege Dateien in einen Eingang – hier, in der Dateiansicht oder über das Netzlaufwerk.
              </Empty>
            )
          )}
        </>
      )}
    </Box>
  )
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Box sx={{ mb: 2 }}>
      <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
        {title}
      </Typography>
      {children}
    </Box>
  )
}

/** Ein- oder ausgeschaltet und Zustand der KI */
function StatusLine({ data, admin }: { data: InboxStatus; admin: boolean }) {
  if (!data.enabled) {
    return (
      <Alert
        severity="info"
        sx={{ mb: 2 }}
        action={
          admin && (
            <Button color="inherit" size="small" href={hrefFor({ view: 'settings', tab: 'allgemein' })}>
              Einstellungen
            </Button>
          )
        }
      >
        Das automatische Sortieren ist vom Admin ausgeschaltet. Dateien bleiben im Eingang liegen.
      </Alert>
    )
  }
  const { ai } = data
  if (!ai) {
    return (
      <Alert severity="info" sx={{ mb: 2 }}>
        Keine KI eingerichtet – Fotos werden sortiert, Dokumente bleiben im Eingang.
      </Alert>
    )
  }
  const st = ai.status
  switch (st.state) {
    case 'ready':
      return <Chip icon={<AutoAwesomeRoundedIcon />} color="success" variant="outlined" label={`KI bereit · ${ai.model}`} sx={{ mb: 2 }} />
    case 'unknown':
      return <Chip icon={<AutoAwesomeRoundedIcon />} variant="outlined" label={`KI wird geprüft · ${ai.model}`} sx={{ mb: 2 }} />
    case 'unreachable':
      return (
        <Alert severity="warning" sx={{ mb: 2 }}>
          KI ({ai.model}) nicht erreichbar: {st.error}. Dokumente warten, Fotos werden weiter sortiert.
        </Alert>
      )
    case 'pulling': {
      const pct = st.total ? Math.min(100, (st.done / st.total) * 100) : 0
      return (
        <Box sx={{ mb: 2, maxWidth: 480 }}>
          <Typography variant="body2" sx={{ mb: 0.5 }}>
            KI-Modell {ai.model} wird geladen … {st.total ? `${Math.round(pct)} %` : ''}
          </Typography>
          <LinearProgress variant={st.total ? 'determinate' : 'indeterminate'} value={pct} sx={{ borderRadius: 1 }} />
        </Box>
      )
    }
  }
}

function ItemRow({ item, undoing, onUndo }: { item: InboxItem; undoing: boolean; onUndo: () => void }) {
  let status: ReactNode
  switch (item.status) {
    case 'done':
      status = 'einsortiert'
      break
    case 'undone':
      status = 'zurückgelegt nach Eingang/Zurückgelegt'
      break
    case 'error':
      status = (
        <Box component="span" sx={{ color: 'error.main' }}>
          Fehler{item.message ? `: ${item.message}` : ''}
        </Box>
      )
      break
    case 'skipped':
      status = `übersprungen${item.message ? `: ${item.message}` : ''}`
      break
  }
  const target = item.new_path
  return (
    <ListItem
      disableGutters
      secondaryAction={
        item.status === 'done' && (
          <Button
            size="small"
            startIcon={undoing ? <CircularProgress size={16} /> : <UndoRoundedIcon />}
            disabled={undoing}
            onClick={onUndo}
          >
            Rückgängig
          </Button>
        )
      }
      sx={{ borderRadius: 2, alignItems: 'flex-start', '&:hover': { bgcolor: 'action.hover' } }}
    >
      <ListItemAvatar>
        <Avatar variant="rounded" sx={{ bgcolor: 'action.hover', color: 'text.secondary' }}>
          <KindIcon kind={item.kind} />
        </Avatar>
      </ListItemAvatar>
      <ListItemText
        primary={item.original_name}
        secondary={
          <>
            {target && (
              <Box component="span" sx={{ display: 'flex', alignItems: 'center', gap: 0.5, minWidth: 0 }}>
                <ArrowForwardRoundedIcon sx={{ fontSize: 16, flexShrink: 0 }} />
                <Link
                  component="button"
                  variant="body2"
                  underline="hover"
                  title={target}
                  onClick={() => navigate({ view: 'files', path: parentPath(target) })}
                  sx={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', textAlign: 'left' }}
                >
                  {target}
                </Link>
              </Box>
            )}
            <Box component="span" sx={{ display: 'block' }}>
              <span title={dateTime(item.created_at)}>{ago(item.created_at)}</span> · {spaceOf(item.area)} · {status}
            </Box>
          </>
        }
        slotProps={{ primary: { noWrap: true }, secondary: { component: 'span' } }}
        sx={{ pr: item.status === 'done' ? 16 : 0, minWidth: 0 }}
      />
    </ListItem>
  )
}
