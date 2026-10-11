import type { ReactNode } from 'react'
import { Box, CircularProgress, Stack, Typography } from '@mui/material'

/** Kopfzeile einer Ansicht: Titel links, Aktionen rechts. */
export function ViewHeader({ title, subtitle, actions }: { title: ReactNode; subtitle?: ReactNode; actions?: ReactNode }) {
  return (
    <Stack direction="row" sx={{ alignItems: 'center', mb: 2, minHeight: 44, gap: 1 }}>
      <Box sx={{ flex: 1, minWidth: 0 }}>
        {typeof title === 'string' ? (
          <Typography variant="h5" noWrap sx={{ fontWeight: 700 }}>
            {title}
          </Typography>
        ) : (
          title
        )}
        {subtitle && (
          <Typography variant="body2" color="text.secondary">
            {subtitle}
          </Typography>
        )}
      </Box>
      {actions}
    </Stack>
  )
}

export function Loading() {
  return (
    <Box sx={{ display: 'grid', placeItems: 'center', py: 10 }}>
      <CircularProgress />
    </Box>
  )
}

export function Empty({ icon, children, action }: { icon: ReactNode; children: ReactNode; action?: ReactNode }) {
  return (
    <Stack sx={{ alignItems: 'center', py: 10, gap: 2, color: 'text.secondary', textAlign: 'center' }}>
      <Box sx={{ fontSize: 72, opacity: 0.5, '& svg': { fontSize: 'inherit' } }}>{icon}</Box>
      <Typography sx={{ maxWidth: 420 }}>{children}</Typography>
      {action}
    </Stack>
  )
}
