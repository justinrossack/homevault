import type { ReactNode } from 'react'
import { AppBar, IconButton, Toolbar, Tooltip, Typography } from '@mui/material'
import CloseRoundedIcon from '@mui/icons-material/CloseRounded'
import FavoriteBorderRoundedIcon from '@mui/icons-material/FavoriteBorderRounded'
import FavoriteRoundedIcon from '@mui/icons-material/FavoriteRounded'
import PhotoAlbumOutlinedIcon from '@mui/icons-material/PhotoAlbumOutlined'
import DeleteOutlineRoundedIcon from '@mui/icons-material/DeleteOutlineRounded'
import type { Entry } from '../api'
import { useShell } from '../shell'

interface Props {
  selected: Entry[]
  onClear: () => void
  /** Nach Favoriten-Änderung (zum Aktualisieren der Ansicht) */
  onFavorite?: (entries: Entry[], favorite: boolean) => void
  /** Weitere Aktionen, z. B. „Aus Album entfernen“ */
  extra?: ReactNode
  /** Nach dem Löschen (Pfade der gelöschten Dateien) */
  onDeleted?: (paths: Set<string>) => void
}

/** Leiste über der App-Leiste, solange etwas ausgewählt ist (wie in Google Fotos). */
export default function SelectionBar({ selected, onClear, onFavorite, extra, onDeleted }: Props) {
  const shell = useShell()
  if (selected.length === 0) return null
  const allFav = selected.every((e) => e.favorite)

  return (
    <AppBar position="fixed" color="primary" elevation={4} sx={{ zIndex: (t) => t.zIndex.drawer + 2 }}>
      <Toolbar sx={{ gap: 1 }}>
        <IconButton color="inherit" edge="start" onClick={onClear} aria-label="Auswahl aufheben">
          <CloseRoundedIcon />
        </IconButton>
        <Typography sx={{ flex: 1, fontWeight: 600 }}>{selected.length} ausgewählt</Typography>
        <Tooltip title={allFav ? 'Aus Favoriten entfernen' : 'Zu Favoriten'}>
          <IconButton
            color="inherit"
            onClick={async () => {
              if (await shell.setFavorite(selected, !allFav)) {
                onFavorite?.(selected, !allFav)
                onClear()
              }
            }}
          >
            {allFav ? <FavoriteRoundedIcon /> : <FavoriteBorderRoundedIcon />}
          </IconButton>
        </Tooltip>
        <Tooltip title="Zu Album hinzufügen">
          <IconButton
            color="inherit"
            onClick={() => {
              shell.addToAlbum(selected)
              onClear()
            }}
          >
            <PhotoAlbumOutlinedIcon />
          </IconButton>
        </Tooltip>
        {extra}
        {onDeleted && (
          <Tooltip title="Löschen">
            <IconButton color="inherit" onClick={() => shell.deleteEntries(selected, (paths) => (onDeleted(paths), onClear()))}>
              <DeleteOutlineRoundedIcon />
            </IconButton>
          </Tooltip>
        )}
      </Toolbar>
    </AppBar>
  )
}
