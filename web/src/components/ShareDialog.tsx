import { useCallback, useEffect, useState } from 'react'
import {
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Divider,
  FormControlLabel,
  IconButton,
  MenuItem,
  Stack,
  Switch,
  TextField,
  Tooltip,
  Typography,
} from '@mui/material'
import ContentCopyRoundedIcon from '@mui/icons-material/ContentCopyRounded'
import DeleteOutlineRoundedIcon from '@mui/icons-material/DeleteOutlineRounded'
import LinkRoundedIcon from '@mui/icons-material/LinkRounded'
import LockRoundedIcon from '@mui/icons-material/LockRounded'
import { api, type ShareLink } from '../api'

export type ShareTarget = { album_id: number; title: string } | { path: string; title: string }

const filterOf = (t: ShareTarget) => ('album_id' in t ? { album_id: t.album_id } : { path: t.path })

export const fullUrl = (url: string) => `${location.origin}${url}`

/** Link in die Zwischenablage, mit Rückfall für http ohne Clipboard-API. */
export async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text)
  } catch {
    const t = document.createElement('textarea')
    t.value = text
    document.body.appendChild(t)
    t.select()
    document.execCommand('copy')
    t.remove()
  }
}

export function expiryText(s: ShareLink) {
  if (s.expired) return 'abgelaufen'
  if (!s.expires_at) return 'läuft nicht ab'
  return `bis ${new Date(s.expires_at * 1000).toLocaleDateString('de-DE')}`
}

export default function ShareDialog({
  target,
  onClose,
  onToast,
}: {
  target: ShareTarget | null
  onClose: () => void
  onToast: (m: string) => void
}) {
  const [links, setLinks] = useState<ShareLink[] | null>(null)
  const [password, setPassword] = useState('')
  const [days, setDays] = useState(0)
  const [download, setDownload] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')

  const load = useCallback(() => {
    if (!target) return
    api
      .shares(filterOf(target))
      .then((r) => setLinks(r.shares))
      .catch((e) => setError((e as Error).message))
  }, [target])

  useEffect(() => {
    if (!target) return
    setLinks(null)
    setPassword('')
    setDays(0)
    setDownload(true)
    setError('')
    load()
  }, [target, load])

  async function create() {
    if (!target) return
    setBusy(true)
    setError('')
    try {
      const r = await api.createShare({
        ...filterOf(target),
        password: password || undefined,
        expires_days: days || undefined,
        allow_download: download,
      })
      await copyText(fullUrl(r.url))
      onToast('Link erstellt und kopiert')
      setPassword('')
      load()
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open={!!target} onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>„{target?.title}“ teilen</DialogTitle>
      <DialogContent>
        <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
          Jeder mit dem Link kann {target && 'album_id' in target ? 'das Album' : 'die Datei'} ansehen, auch ohne Konto.
          {target && 'album_id' in target && ' Fotos, die du später hinzufügst, sind automatisch dabei.'}
        </Typography>
        {error && (
          <Alert severity="error" sx={{ mb: 2 }}>
            {error}
          </Alert>
        )}

        {links === null ? (
          <CircularProgress size={24} />
        ) : (
          links.length > 0 && (
            <Stack spacing={1} sx={{ mb: 2 }}>
              {links.map((l) => (
                <Stack key={l.id} direction="row" sx={{ alignItems: 'center', gap: 1, p: 1, borderRadius: 2, bgcolor: 'action.hover' }}>
                  <LinkRoundedIcon color={l.expired ? 'disabled' : 'primary'} />
                  <Box sx={{ flex: 1, minWidth: 0 }}>
                    <Typography variant="body2" noWrap sx={{ fontFamily: 'ui-monospace, monospace', fontSize: 12 }}>
                      {fullUrl(l.url)}
                    </Typography>
                    <Stack direction="row" sx={{ gap: 0.5, mt: 0.5, flexWrap: 'wrap' }}>
                      {l.has_password && <Chip size="small" icon={<LockRoundedIcon />} label="Passwort" />}
                      <Chip size="small" label={expiryText(l)} color={l.expired ? 'warning' : 'default'} />
                      {!l.allow_download && <Chip size="small" label="ohne Download" />}
                      <Chip size="small" label={`${l.views}× geöffnet`} />
                    </Stack>
                  </Box>
                  <Tooltip title="Link kopieren">
                    <IconButton onClick={() => copyText(fullUrl(l.url)).then(() => onToast('Link kopiert'))}>
                      <ContentCopyRoundedIcon />
                    </IconButton>
                  </Tooltip>
                  <Tooltip title="Link löschen">
                    <IconButton
                      onClick={() =>
                        api
                          .deleteShare(l.id)
                          .then(() => {
                            onToast('Link gelöscht, er funktioniert nicht mehr')
                            load()
                          })
                          .catch((e) => setError((e as Error).message))
                      }
                    >
                      <DeleteOutlineRoundedIcon />
                    </IconButton>
                  </Tooltip>
                </Stack>
              ))}
            </Stack>
          )
        )}

        <Divider sx={{ mb: 2 }} />
        <Typography variant="subtitle2" sx={{ mb: 1.5 }}>
          Neuen Link erstellen
        </Typography>
        <Stack
          component="form"
          spacing={2}
          onSubmit={(e) => {
            e.preventDefault()
            create()
          }}
        >
          <Stack direction={{ xs: 'column', sm: 'row' }} spacing={1.5}>
            <TextField
              label="Passwort (optional)"
              type="password"
              autoComplete="new-password"
              size="small"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              sx={{ flex: 1 }}
            />
            <TextField select label="Gültig" size="small" value={days} onChange={(e) => setDays(Number(e.target.value))} sx={{ flex: 1 }}>
              <MenuItem value={0}>Unbegrenzt</MenuItem>
              <MenuItem value={1}>1 Tag</MenuItem>
              <MenuItem value={7}>7 Tage</MenuItem>
              <MenuItem value={30}>30 Tage</MenuItem>
              <MenuItem value={365}>1 Jahr</MenuItem>
            </TextField>
          </Stack>
          <FormControlLabel
            control={<Switch checked={download} onChange={(e) => setDownload(e.target.checked)} />}
            label="Herunterladen erlauben"
          />
          <Box>
            <Button type="submit" variant="contained" startIcon={<LinkRoundedIcon />} disabled={busy}>
              Link erstellen und kopieren
            </Button>
          </Box>
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Schließen</Button>
      </DialogActions>
    </Dialog>
  )
}
