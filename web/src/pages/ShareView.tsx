import { useCallback, useEffect, useMemo, useState, type FormEvent } from 'react'
import { Alert, Box, Button, CircularProgress, Container, Paper, Stack, TextField, Typography } from '@mui/material'
import PhotoLibraryRoundedIcon from '@mui/icons-material/PhotoLibraryRounded'
import LockRoundedIcon from '@mui/icons-material/LockRounded'
import LinkOffRoundedIcon from '@mui/icons-material/LinkOffRounded'
import { ApiError, api, shareSource, type Entry, type PublicShare } from '../api'
import FileGrid from '../components/FileGrid'
import Lightbox from '../components/Lightbox'

/** Öffentliche Seite für Teilen-Links (/s/<token>), ohne Anmeldung. */
export default function ShareView({ token }: { token: string }) {
  const [share, setShare] = useState<PublicShare | null>(null)
  const [error, setError] = useState('')
  const [lightbox, setLightbox] = useState<number | null>(null)

  const load = useCallback(() => {
    api
      .publicShare(token)
      .then((s) => {
        setShare(s)
        document.title = s.title ? `${s.title} – ${s.server_name}` : s.server_name
      })
      .catch((e: ApiError) => setError(e.status === 404 ? 'Diesen Link gibt es nicht (mehr).' : e.message))
  }, [token])
  useEffect(load, [load])

  const source = useMemo(() => shareSource(token, share?.allow_download ?? false), [token, share?.allow_download])
  // Einträge in das gewohnte Format bringen (Pfad dient nur als eindeutiger Schlüssel)
  const entries: Entry[] = useMemo(
    () => (share?.entries ?? []).map((e) => ({ ...e, path: `item-${e.id}`, live: e.live ? 'live' : undefined })),
    [share],
  )

  if (error) {
    return (
      <Centered>
        <LinkOffRoundedIcon sx={{ fontSize: 56, color: 'text.secondary' }} />
        <Typography variant="h6">{error}</Typography>
      </Centered>
    )
  }
  if (!share) {
    return (
      <Centered>
        <CircularProgress />
      </Centered>
    )
  }
  if (share.needs_password) return <PasswordGate token={token} serverName={share.server_name} onUnlocked={load} />

  const single = share.kind === 'file' && entries.length === 1
  return (
    <Box sx={{ minHeight: '100dvh', bgcolor: 'background.default' }}>
      <Container maxWidth="xl" sx={{ py: 3 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5, mb: 3 }}>
          <PhotoLibraryRoundedIcon color="primary" sx={{ fontSize: 32 }} />
          <Box sx={{ flex: 1, minWidth: 0 }}>
            <Typography variant="h5" noWrap sx={{ fontWeight: 700 }}>
              {share.title}
            </Typography>
            <Typography variant="body2" color="text.secondary">
              Geteilt von {share.owner} über {share.server_name}
              {share.kind === 'album' && ` · ${entries.length} ${entries.length === 1 ? 'Element' : 'Elemente'}`}
              {share.expires_at && ` · gültig bis ${new Date(share.expires_at * 1000).toLocaleDateString('de-DE')}`}
            </Typography>
          </Box>
          {single && source.download(entries[0]) && (
            <Button variant="contained" href={source.download(entries[0])!}>
              Herunterladen
            </Button>
          )}
        </Stack>

        {entries.length === 0 ? (
          <Typography color="text.secondary">Hier ist (noch) nichts drin.</Typography>
        ) : (
          <FileGrid entries={entries} compact onOpen={(e) => setLightbox(entries.indexOf(e))} source={source} />
        )}
      </Container>
      <Lightbox items={entries} index={lightbox} onIndex={setLightbox} source={source} />
    </Box>
  )
}

function Centered({ children }: { children: React.ReactNode }) {
  return (
    <Stack sx={{ minHeight: '100dvh', alignItems: 'center', justifyContent: 'center', gap: 2, p: 3, textAlign: 'center' }}>
      {children}
    </Stack>
  )
}

function PasswordGate({ token, serverName, onUnlocked }: { token: string; serverName: string; onUnlocked: () => void }) {
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  async function submit(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError('')
    try {
      await api.unlockShare(token, password)
      onUnlocked()
    } catch (err) {
      setError((err as Error).message)
      setBusy(false)
    }
  }

  return (
    <Centered>
      <Paper component="form" onSubmit={submit} variant="outlined" sx={{ p: 4, width: '100%', maxWidth: 380 }}>
        <Stack spacing={2.5} sx={{ alignItems: 'center' }}>
          <LockRoundedIcon color="primary" sx={{ fontSize: 44 }} />
          <Typography variant="h6">Geschützter Link</Typography>
          <Typography variant="body2" color="text.secondary">
            Bitte gib das Passwort ein, das du von der Person bekommen hast, die {serverName} betreibt.
          </Typography>
          {error && (
            <Alert severity="error" sx={{ width: '100%' }}>
              {error}
            </Alert>
          )}
          <TextField label="Passwort" type="password" autoFocus fullWidth value={password} onChange={(e) => setPassword(e.target.value)} />
          <Button type="submit" variant="contained" size="large" fullWidth disabled={busy || !password}>
            Öffnen
          </Button>
        </Stack>
      </Paper>
    </Centered>
  )
}
