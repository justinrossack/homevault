import { useEffect, useState } from 'react'
import {
  Alert,
  Box,
  Breadcrumbs,
  Button,
  ButtonBase,
  Checkbox,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  FormControlLabel,
  Link,
  MenuItem,
  Radio,
  RadioGroup,
  Stack,
  TextField,
  Typography,
} from '@mui/material'
import FolderRoundedIcon from '@mui/icons-material/FolderRounded'
import RestoreRoundedIcon from '@mui/icons-material/RestoreRounded'
import NavigateNextIcon from '@mui/icons-material/NavigateNext'
import { api, formatSize, type BackupItem, type BackupTarget, type Entry } from '../api'
import { FileTypeIcon } from './FileGrid'

/** Im Backup blättern und Dateien/Ordner zurückholen. */
export default function RestoreDialog({
  target,
  onClose,
  onStarted,
}: {
  target: BackupTarget | null
  onClose: () => void
  onStarted: () => void
}) {
  const [versions, setVersions] = useState<{ id: string; label: string }[]>([])
  const [version, setVersion] = useState('')
  const [path, setPath] = useState('')
  const [items, setItems] = useState<BackupItem[] | null>(null)
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [mode, setMode] = useState<'original' | 'folder'>('original')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  // Neues Ziel: Zustand sofort zurücksetzen, bevor geladen wird (nicht erst im Effekt)
  const targetId = target?.id ?? null
  const [shownId, setShownId] = useState(targetId)
  if (shownId !== targetId) {
    setShownId(targetId)
    // beim Schließen stehen lassen, sonst flackert der Inhalt
    if (targetId !== null) {
      setVersions([])
      setVersion('')
      setPath('')
      setItems(null)
      setSelected(new Set())
      setError('')
    }
  }

  useEffect(() => {
    if (targetId === null) return
    let cancelled = false
    api
      .backupVersions(targetId)
      .then((r) => !cancelled && setVersions(r.versions))
      .catch((e) => !cancelled && setError((e as Error).message))
    return () => {
      cancelled = true
    }
  }, [targetId])

  useEffect(() => {
    if (targetId === null) return
    // veraltete Antworten (anderer Ordner/Stand) verwerfen
    let cancelled = false
    setItems(null)
    api
      .backupBrowse(targetId, version, path)
      .then((r) => !cancelled && setItems(r.entries))
      .catch((e) => {
        if (cancelled) return
        setItems([])
        setError((e as Error).message)
      })
    return () => {
      cancelled = true
    }
  }, [targetId, version, path])

  async function start() {
    if (!target) return
    setBusy(true)
    setError('')
    try {
      await api.backupRestore(target.id, { version, paths: [...selected], mode })
      onStarted()
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  const parts = path ? path.split('/') : []
  const toggle = (p: string) =>
    setSelected((s) => {
      const n = new Set(s)
      if (n.has(p)) n.delete(p)
      else n.add(p)
      return n
    })

  return (
    <Dialog open={!!target} onClose={onClose} fullWidth maxWidth="md">
      <DialogTitle>Aus „{target?.name}“ wiederherstellen</DialogTitle>
      <DialogContent>
        {error && (
          <Alert severity="error" sx={{ mb: 2 }} onClose={() => setError('')}>
            {error}
          </Alert>
        )}
        <TextField
          select
          size="small"
          label="Stand"
          value={version}
          onChange={(e) => {
            setVersion(e.target.value)
            setPath('')
            setSelected(new Set())
          }}
          sx={{ mt: 1, mb: 2, minWidth: 320 }}
          helperText={version ? 'Dateien, die zu diesem Zeitpunkt gelöscht oder geändert wurden' : 'So wie beim letzten Backup'}
        >
          {versions.map((v) => (
            <MenuItem key={v.id} value={v.id}>
              {v.label}
            </MenuItem>
          ))}
        </TextField>

        <Breadcrumbs separator={<NavigateNextIcon fontSize="small" />} sx={{ mb: 1 }}>
          <Link component="button" underline="hover" color={parts.length ? 'inherit' : 'text.primary'} onClick={() => setPath('')}>
            Backup
          </Link>
          {parts.map((p, i) => (
            <Link
              key={i}
              component="button"
              underline="hover"
              color={i === parts.length - 1 ? 'text.primary' : 'inherit'}
              onClick={() => setPath(parts.slice(0, i + 1).join('/'))}
            >
              {p}
            </Link>
          ))}
        </Breadcrumbs>

        <Box sx={{ border: 1, borderColor: 'divider', borderRadius: 2, minHeight: 240, maxHeight: 360, overflow: 'auto' }}>
          {items === null ? (
            <Box sx={{ display: 'grid', placeItems: 'center', py: 6 }}>
              <CircularProgress size={28} />
            </Box>
          ) : items.length === 0 ? (
            <Typography color="text.secondary" sx={{ p: 3, textAlign: 'center' }}>
              Hier ist nichts.
            </Typography>
          ) : (
            items.map((it) => {
              const fake: Entry = { name: it.name, path: it.path, kind: 'other', size: it.size ?? 0, modified: 0 }
              return (
                <Stack key={it.path} direction="row" sx={{ alignItems: 'center', gap: 1, px: 1, borderBottom: 1, borderColor: 'divider' }}>
                  {/* auf oberster Ebene ganze Bereiche nicht auswählbar: erst hineinklicken */}
                  <Checkbox checked={selected.has(it.path)} onChange={() => toggle(it.path)} disabled={!path} />
                  <ButtonBase
                    onClick={() => (it.dir ? setPath(it.path) : toggle(it.path))}
                    sx={{ flex: 1, justifyContent: 'flex-start', gap: 1.5, py: 1, textAlign: 'left', borderRadius: 1, minWidth: 0 }}
                  >
                    <Box sx={{ display: 'flex', '& svg': { fontSize: 24 } }}>
                      {it.dir ? <FolderRoundedIcon color="primary" /> : <FileTypeIcon entry={fake} />}
                    </Box>
                    <Typography noWrap sx={{ flex: 1 }}>
                      {it.name}
                    </Typography>
                    {!it.dir && it.size != null && (
                      <Typography variant="body2" color="text.secondary" sx={{ pr: 1 }}>
                        {formatSize(it.size)}
                      </Typography>
                    )}
                  </ButtonBase>
                </Stack>
              )
            })
          )}
        </Box>

        <Typography variant="subtitle2" sx={{ mt: 2.5 }}>
          Wohin?
        </Typography>
        <RadioGroup value={mode} onChange={(e) => setMode(e.target.value as 'original' | 'folder')}>
          <FormControlLabel
            value="original"
            control={<Radio />}
            label="An den ursprünglichen Ort – vorhandene Dateien bleiben unverändert"
          />
          <FormControlLabel
            value="folder"
            control={<Radio />}
            label="In einen neuen Ordner „Wiederhergestellt …“ – nichts wird vermischt"
          />
        </RadioGroup>
      </DialogContent>
      <DialogActions>
        <Typography variant="body2" color="text.secondary" sx={{ flex: 1, pl: 2 }}>
          {selected.size > 0 ? `${selected.size} ausgewählt` : 'Ordner öffnen und Dateien oder Ordner auswählen'}
        </Typography>
        <Button onClick={onClose}>Abbrechen</Button>
        <Button variant="contained" startIcon={<RestoreRoundedIcon />} disabled={busy || selected.size === 0} onClick={start}>
          Wiederherstellen
        </Button>
      </DialogActions>
    </Dialog>
  )
}
