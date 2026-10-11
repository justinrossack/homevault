import { Box, IconButton, LinearProgress, Paper, Stack, Typography } from '@mui/material'
import CloseRoundedIcon from '@mui/icons-material/CloseRounded'
import CheckCircleRoundedIcon from '@mui/icons-material/CheckCircleRounded'
import ErrorRoundedIcon from '@mui/icons-material/ErrorRounded'

export interface UploadItem {
  id: number
  name: string
  progress: number
  status: 'waiting' | 'uploading' | 'done' | 'error'
  error?: string
}

export default function UploadPanel({ items, onClose }: { items: UploadItem[]; onClose: () => void }) {
  if (items.length === 0) return null
  const done = items.filter((i) => i.status === 'done').length
  const running = items.some((i) => i.status === 'waiting' || i.status === 'uploading')

  return (
    <Paper
      elevation={8}
      sx={{
        position: 'fixed',
        right: { xs: 8, sm: 24 },
        bottom: { xs: 8, sm: 24 },
        left: { xs: 8, sm: 'auto' },
        width: { sm: 360 },
        maxHeight: 360,
        display: 'flex',
        flexDirection: 'column',
        overflow: 'hidden',
        zIndex: (t) => t.zIndex.snackbar,
      }}
    >
      <Stack direction="row" sx={{ alignItems: 'center', px: 2, py: 1, borderBottom: 1, borderColor: 'divider' }}>
        <Typography sx={{ flex: 1, fontWeight: 600 }}>
          {running ? `Lade hoch … ${done}/${items.length}` : `${done} von ${items.length} hochgeladen`}
        </Typography>
        {!running && (
          <IconButton size="small" onClick={onClose} aria-label="Schließen">
            <CloseRoundedIcon fontSize="small" />
          </IconButton>
        )}
      </Stack>
      <Box sx={{ overflow: 'auto', px: 2, py: 1 }}>
        {items.map((i) => (
          <Box key={i.id} sx={{ py: 0.75 }}>
            <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
              <Typography variant="body2" noWrap sx={{ flex: 1 }} title={i.name}>
                {i.name}
              </Typography>
              {i.status === 'done' && <CheckCircleRoundedIcon color="success" fontSize="small" />}
              {i.status === 'error' && <ErrorRoundedIcon color="error" fontSize="small" />}
            </Stack>
            {i.status === 'error' ? (
              <Typography variant="caption" color="error">
                {i.error}
              </Typography>
            ) : (
              i.status !== 'done' && (
                <LinearProgress
                  variant={i.status === 'waiting' ? 'indeterminate' : 'determinate'}
                  value={i.progress * 100}
                  sx={{ mt: 0.5, borderRadius: 1, opacity: i.status === 'waiting' ? 0.4 : 1 }}
                />
              )
            )}
          </Box>
        ))}
      </Box>
    </Paper>
  )
}
