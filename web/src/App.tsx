import { useCallback, useEffect, useState } from 'react'
import { Box, CircularProgress } from '@mui/material'
import { api, setUnauthorizedHandler, type Me } from './api'
import Login from './pages/Login'
import Setup from './pages/Setup'
import Shell from './pages/Shell'
import ShareView from './pages/ShareView'

// Teilen-Links (/s/<token>) brauchen keine Anmeldung
const shareToken = location.pathname.match(/^\/s\/([0-9a-f]+)\/?$/)?.[1]

export default function App() {
  if (shareToken) return <ShareView token={shareToken} />
  return <MainApp />
}

function MainApp() {
  const [me, setMe] = useState<Me | null>(null)
  const [error, setError] = useState(false)

  const refresh = useCallback(
    () =>
      api
        .me()
        .then((r) => {
          setMe(r)
          setError(false)
          document.title = r.server_name
        })
        .catch(() => setError(true)),
    [],
  )

  useEffect(() => {
    setUnauthorizedHandler(() => setMe((m) => (m ? { ...m, authenticated: false, user: null } : m)))
    refresh()
  }, [refresh])

  if (!me) {
    return (
      <Box sx={{ display: 'grid', placeItems: 'center', height: '100dvh', color: 'text.secondary' }}>
        {error ? 'Server nicht erreichbar' : <CircularProgress />}
      </Box>
    )
  }
  if (me.setup_needed) return <Setup onDone={refresh} />
  if (!me.user) return <Login serverName={me.server_name} onLogin={refresh} />
  return (
    <Shell
      user={me.user}
      serverName={me.server_name}
      refreshMe={refresh}
      onLogout={() => setMe({ ...me, authenticated: false, user: null })}
    />
  )
}
