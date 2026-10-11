import { useState, type FormEvent } from 'react'
import { Alert, Box, Button, Paper, Stack, TextField, Typography } from '@mui/material'
import PhotoLibraryRoundedIcon from '@mui/icons-material/PhotoLibraryRounded'
import { api, ApiError } from '../api'

export default function Login({ serverName, onLogin }: { serverName: string; onLogin: () => void }) {
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  async function submit(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError('')
    try {
      await api.login(username, password)
      onLogin()
    } catch (e) {
      setError(e instanceof ApiError && e.status === 401 ? 'Benutzername oder Passwort falsch' : (e as Error).message)
      setBusy(false)
    }
  }

  return (
    <Box sx={{ display: 'grid', placeItems: 'center', minHeight: '100dvh', p: 2 }}>
      <Paper component="form" onSubmit={submit} elevation={0} variant="outlined" sx={{ p: 4, width: '100%', maxWidth: 380 }}>
        <Stack spacing={3} sx={{ alignItems: 'center' }}>
          <PhotoLibraryRoundedIcon color="primary" sx={{ fontSize: 48 }} />
          <Typography variant="h5" sx={{ fontWeight: 700 }}>
            {serverName}
          </Typography>
          {error && (
            <Alert severity="error" sx={{ width: '100%' }}>
              {error}
            </Alert>
          )}
          <TextField
            label="Benutzername"
            autoFocus
            fullWidth
            autoComplete="username"
            autoCapitalize="none"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
          />
          <TextField
            label="Passwort"
            type="password"
            fullWidth
            autoComplete="current-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
          <Button type="submit" variant="contained" size="large" fullWidth disabled={busy || !username || !password}>
            Anmelden
          </Button>
        </Stack>
      </Paper>
    </Box>
  )
}
