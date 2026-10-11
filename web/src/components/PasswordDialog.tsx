import { useEffect, useState } from 'react'
import { Alert, Button, Dialog, DialogActions, DialogContent, DialogTitle, Stack, TextField } from '@mui/material'
import { api } from '../api'

export default function PasswordDialog({ open, onClose, onDone }: { open: boolean; onClose: () => void; onDone: () => void }) {
  const [current, setCurrent] = useState('')
  const [next, setNext] = useState('')
  const [repeat, setRepeat] = useState('')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    if (open) {
      setCurrent('')
      setNext('')
      setRepeat('')
      setError('')
    }
  }, [open])

  const mismatch = repeat.length > 0 && next !== repeat

  async function submit() {
    setBusy(true)
    setError('')
    try {
      await api.changePassword(current, next)
      onDone()
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open={open} onClose={onClose} fullWidth maxWidth="xs">
      <form
        onSubmit={(e) => {
          e.preventDefault()
          submit()
        }}
      >
        <DialogTitle>Passwort ändern</DialogTitle>
        <DialogContent>
          <Stack spacing={2} sx={{ pt: 1 }}>
            {error && <Alert severity="error">{error}</Alert>}
            <TextField
              label="Aktuelles Passwort"
              type="password"
              autoComplete="current-password"
              autoFocus
              value={current}
              onChange={(e) => setCurrent(e.target.value)}
            />
            <TextField
              label="Neues Passwort"
              type="password"
              autoComplete="new-password"
              value={next}
              onChange={(e) => setNext(e.target.value)}
            />
            <TextField
              label="Neues Passwort wiederholen"
              type="password"
              autoComplete="new-password"
              value={repeat}
              onChange={(e) => setRepeat(e.target.value)}
              error={mismatch}
              helperText={mismatch ? 'Passwörter stimmen nicht überein' : 'Gilt auch für das Netzlaufwerk (SMB)'}
            />
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={onClose}>Abbrechen</Button>
          <Button type="submit" variant="contained" disabled={busy || !current || !next || next !== repeat}>
            Ändern
          </Button>
        </DialogActions>
      </form>
    </Dialog>
  )
}
