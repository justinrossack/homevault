import { Breadcrumbs, Link, Typography } from '@mui/material'
import NavigateNextIcon from '@mui/icons-material/NavigateNext'

export default function PathBar({ path, onNavigate }: { path: string; onNavigate: (p: string) => void }) {
  const parts = path ? path.split('/') : []
  return (
    <Breadcrumbs separator={<NavigateNextIcon fontSize="small" />} maxItems={5} sx={{ minWidth: 0 }}>
      {parts.length === 0 ? (
        <Typography sx={{ fontWeight: 600 }} color="text.primary">
          Alle Dateien
        </Typography>
      ) : (
        <Link component="button" underline="hover" color="inherit" onClick={() => onNavigate('')}>
          Alle Dateien
        </Link>
      )}
      {parts.map((p, i) => {
        const target = parts.slice(0, i + 1).join('/')
        return i === parts.length - 1 ? (
          <Typography key={target} sx={{ fontWeight: 600 }} color="text.primary" noWrap>
            {p}
          </Typography>
        ) : (
          <Link key={target} component="button" underline="hover" color="inherit" onClick={() => onNavigate(target)}>
            {p}
          </Link>
        )
      })}
    </Breadcrumbs>
  )
}
