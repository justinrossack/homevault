import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Box, IconButton, ListItemIcon, ListItemText, Menu, MenuItem, Tooltip, Typography } from '@mui/material'
import FavoriteRoundedIcon from '@mui/icons-material/FavoriteRounded'
import FavoriteBorderRoundedIcon from '@mui/icons-material/FavoriteBorderRounded'
import PhotoAlbumOutlinedIcon from '@mui/icons-material/PhotoAlbumOutlined'
import ShareRoundedIcon from '@mui/icons-material/ShareRounded'
import DownloadRoundedIcon from '@mui/icons-material/DownloadRounded'
import AdjustRoundedIcon from '@mui/icons-material/AdjustRounded'
import FolderOpenRoundedIcon from '@mui/icons-material/FolderOpenRounded'
import DeleteOutlineRoundedIcon from '@mui/icons-material/DeleteOutlineRounded'
import { api, formatTaken, parentPath, type Entry } from '../api'
import { navigate } from '../router'
import { useShell } from '../shell'
import FileGrid from './FileGrid'
import Lightbox from './Lightbox'
import SelectionBar from './SelectionBar'
import MarqueeArea from './MarqueeArea'
import { useSelection } from '../hooks/useSelection'

export interface ExtraAction {
  label: string
  icon: ReactNode
  run: (entries: Entry[]) => void
}

interface Props {
  entries: Entry[]
  setEntries: (fn: (prev: Entry[]) => Entry[]) => void
  /** Nach Monaten gruppieren (Zeitleiste) */
  groupByMonth?: boolean
  /** Einträge verschwinden, wenn sie kein Favorit mehr sind (Favoriten-Ansicht) */
  removeWhenUnfavorited?: boolean
  /** Zusätzliche Aktion in Menü und Auswahlleiste, z. B. „Aus Album entfernen“ */
  extraAction?: ExtraAction
  /** Wird aufgerufen, wenn das Ende sichtbar wird (Nachladen) */
  onEndReached?: () => void
}

/** Raster mit Lightbox, Auswahl und Menü für Fotos aus der Bibliothek. */
export default function MediaCollection({ entries, setEntries, groupByMonth, removeWhenUnfavorited, extraAction, onEndReached }: Props) {
  const shell = useShell()
  const [lightbox, setLightbox] = useState<number | null>(null)
  const [menu, setMenu] = useState<{ entry: Entry; anchor: HTMLElement } | null>(null)
  const { selected, selectedEntries, toggle, clear, beginMarquee, marquee } = useSelection(entries)

  const applyFavorite = useCallback(
    (changed: Entry[], favorite: boolean) => {
      const paths = new Set(changed.map((e) => e.path))
      setEntries((prev) =>
        removeWhenUnfavorited && !favorite
          ? prev.filter((e) => !paths.has(e.path))
          : prev.map((e) => (paths.has(e.path) ? { ...e, favorite } : e)),
      )
    },
    [setEntries, removeWhenUnfavorited],
  )

  const toggleFavorite = useCallback(
    async (e: Entry) => {
      if (await shell.setFavorite([e], !e.favorite)) {
        // in der Lightbox bleiben, auch wenn der Eintrag aus den Favoriten fällt
        if (removeWhenUnfavorited && e.favorite) setLightbox(null)
        applyFavorite([e], !e.favorite)
      }
    },
    [shell, applyFavorite, removeWhenUnfavorited],
  )

  const open = useCallback((e: Entry) => setLightbox(entries.indexOf(e)), [entries])
  const openMenu = useCallback((entry: Entry, anchor: HTMLElement) => setMenu({ entry, anchor }), [])
  // Gelöschtes aus der Ansicht nehmen; Lightbox bleibt beim nächsten Bild
  const removeDeleted = useCallback(
    (paths: Set<string>) => {
      setEntries((prev) => {
        const next = prev.filter((e) => !paths.has(e.path))
        setLightbox((i) => (i === null || next.length === 0 ? null : Math.min(i, next.length - 1)))
        return next
      })
    },
    [setEntries],
  )
  const askDelete = useCallback((items: Entry[]) => shell.deleteEntries(items, removeDeleted), [shell, removeDeleted])

  const groups = useMemo(() => {
    if (!groupByMonth) return [{ key: 'all', label: '', items: entries }]
    const out: { key: string; label: string; items: Entry[] }[] = []
    for (const e of entries) {
      const t = e.taken ?? e.modified
      const key = formatTaken(t, { year: 'numeric', month: '2-digit' })
      if (out.at(-1)?.key !== key) out.push({ key, label: formatTaken(t, { month: 'long', year: 'numeric' }), items: [] })
      out.at(-1)!.items.push(e)
    }
    return out
  }, [entries, groupByMonth])

  // Nachladen, sobald das Ende in Sichtweite kommt
  const sentinel = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!onEndReached || !sentinel.current) return
    const obs = new IntersectionObserver((list) => list[0].isIntersecting && onEndReached(), { rootMargin: '800px' })
    obs.observe(sentinel.current)
    return () => obs.disconnect()
  }, [onEndReached, entries.length])

  const item = (icon: ReactNode, label: string, run: () => void, props: object = {}) => (
    <MenuItem
      {...props}
      onClick={() => {
        run()
        setMenu(null)
      }}
    >
      <ListItemIcon>{icon}</ListItemIcon>
      <ListItemText>{label}</ListItemText>
    </MenuItem>
  )

  return (
    <>
      <SelectionBar
        selected={selectedEntries}
        onClear={clear}
        onFavorite={applyFavorite}
        onDeleted={removeDeleted}
        extra={
          extraAction && (
            <Tooltip title={extraAction.label}>
              <IconButton
                color="inherit"
                onClick={() => {
                  extraAction.run(selectedEntries)
                  clear()
                }}
              >
                {extraAction.icon}
              </IconButton>
            </Tooltip>
          )
        }
      />

      <MarqueeArea onStart={beginMarquee} onSelect={marquee}>
        {groups.map((g) => (
          <Box key={g.key} sx={{ mb: 3 }}>
            {g.label && (
              <Typography
                variant="subtitle1"
                sx={{
                  fontWeight: 700,
                  py: 1,
                  position: 'sticky',
                  top: { xs: 56, sm: 64 },
                  zIndex: 1,
                  bgcolor: 'background.default',
                }}
              >
                {g.label}
              </Typography>
            )}
            <FileGrid entries={g.items} compact onOpen={open} onMenu={openMenu} selected={selected} onToggleSelect={toggle} />
          </Box>
        ))}
        <div ref={sentinel} />
      </MarqueeArea>

      <Menu anchorEl={menu?.anchor} open={!!menu} onClose={() => setMenu(null)}>
        {menu &&
          item(
            menu.entry.favorite ? <FavoriteRoundedIcon fontSize="small" /> : <FavoriteBorderRoundedIcon fontSize="small" />,
            menu.entry.favorite ? 'Aus Favoriten entfernen' : 'Zu Favoriten',
            () => toggleFavorite(menu.entry),
          )}
        {menu && item(<PhotoAlbumOutlinedIcon fontSize="small" />, 'Zu Album hinzufügen', () => shell.addToAlbum([menu.entry]))}
        {menu &&
          item(<ShareRoundedIcon fontSize="small" />, 'Teilen', () => shell.share({ path: menu.entry.path, title: menu.entry.name }))}
        {menu && extraAction && item(extraAction.icon, extraAction.label, () => extraAction.run([menu.entry]))}
        {menu &&
          item(<FolderOpenRoundedIcon fontSize="small" />, 'Im Ordner anzeigen', () =>
            navigate({ view: 'files', path: parentPath(menu.entry.path) }),
          )}
        {menu &&
          item(<DownloadRoundedIcon fontSize="small" />, 'Herunterladen', () => {}, {
            component: 'a',
            href: api.downloadUrl(menu.entry.path),
          })}
        {menu?.entry.live &&
          item(<AdjustRoundedIcon fontSize="small" />, 'Live-Video herunterladen', () => {}, {
            component: 'a',
            href: api.downloadUrl(menu.entry.live),
          })}
        {menu &&
          item(<DeleteOutlineRoundedIcon fontSize="small" color="error" />, 'Löschen', () => askDelete([menu.entry]), {
            sx: { color: 'error.main' },
          })}
      </Menu>

      <Lightbox
        items={entries}
        index={lightbox}
        onIndex={setLightbox}
        onToggleFavorite={toggleFavorite}
        onAddToAlbum={(e) => shell.addToAlbum([e])}
        onShare={(e) => shell.share({ path: e.path, title: e.name })}
        onDelete={(e) => askDelete([e])}
      />
    </>
  )
}
