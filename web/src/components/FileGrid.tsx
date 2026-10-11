import { memo, useRef, useState } from 'react'
import { Box, ButtonBase, IconButton, Typography } from '@mui/material'
import FolderRoundedIcon from '@mui/icons-material/FolderRounded'
import InsertDriveFileOutlinedIcon from '@mui/icons-material/InsertDriveFileOutlined'
import PlayCircleFilledRoundedIcon from '@mui/icons-material/PlayCircleFilledRounded'
import MovieOutlinedIcon from '@mui/icons-material/MovieOutlined'
import ImageOutlinedIcon from '@mui/icons-material/ImageOutlined'
import PersonRoundedIcon from '@mui/icons-material/PersonRounded'
import GroupsRoundedIcon from '@mui/icons-material/GroupsRounded'
import MoreVertIcon from '@mui/icons-material/MoreVert'
import AdjustRoundedIcon from '@mui/icons-material/AdjustRounded'
import FavoriteRoundedIcon from '@mui/icons-material/FavoriteRounded'
import CheckCircleRoundedIcon from '@mui/icons-material/CheckCircleRounded'
import RadioButtonUncheckedRoundedIcon from '@mui/icons-material/RadioButtonUncheckedRounded'
import PictureAsPdfRoundedIcon from '@mui/icons-material/PictureAsPdfRounded'
import DescriptionRoundedIcon from '@mui/icons-material/DescriptionRounded'
import TableChartRoundedIcon from '@mui/icons-material/TableChartRounded'
import SlideshowRoundedIcon from '@mui/icons-material/SlideshowRounded'
import ArticleRoundedIcon from '@mui/icons-material/ArticleRounded'
import AudiotrackRoundedIcon from '@mui/icons-material/AudiotrackRounded'
import FolderZipRoundedIcon from '@mui/icons-material/FolderZipRounded'
import { formatSize, isMedia, userSource, type Entry, type MediaSource } from '../api'

interface Props {
  entries: Entry[]
  onOpen: (e: Entry) => void
  onMenu?: (e: Entry, anchor: HTMLElement) => void
  /** Ausgewählte Pfade. Ist etwas ausgewählt, wählt ein Klick statt zu öffnen. */
  selected?: Set<string>
  /** range = ⇧-Klick (Bereich bis zum zuletzt gewählten) */
  onToggleSelect?: (e: Entry, range?: boolean) => void
  /** Nur Bilder ohne Dateinamen (Zeitleiste, Alben) */
  compact?: boolean
  source?: MediaSource
}

export default function FileGrid({ entries, compact, ...rest }: Props) {
  return (
    <Box
      sx={{
        display: 'grid',
        gap: compact ? 0.5 : { xs: 1, sm: 1.5 },
        gridTemplateColumns: compact
          ? { xs: 'repeat(auto-fill, minmax(96px, 1fr))', sm: 'repeat(auto-fill, minmax(150px, 1fr))' }
          : { xs: 'repeat(auto-fill, minmax(110px, 1fr))', sm: 'repeat(auto-fill, minmax(170px, 1fr))' },
      }}
    >
      {entries.map((e) => (
        <Tile
          key={e.path}
          entry={e}
          compact={compact}
          isSelected={rest.selected?.has(e.path) ?? false}
          selecting={(rest.selected?.size ?? 0) > 0}
          {...rest}
        />
      ))}
    </Box>
  )
}

type TileProps = Omit<Props, 'entries' | 'selected'> & { entry: Entry; isSelected: boolean; selecting: boolean }

const Tile = memo(function Tile({ entry, onOpen, onMenu, onToggleSelect, isSelected, selecting, compact, source }: TileProps) {
  const [thumbFailed, setThumbFailed] = useState(false)
  const media = isMedia(entry)
  // PDFs und Office-Dateien haben ein Vorschaubild der ersten Seite
  const pageThumb = entry.doc === 'pdf' || entry.doc === 'office'
  // Bereiche in der Übersicht können nicht umbenannt/gelöscht werden
  const isSpace = !entry.path.includes('/')
  const selectable = media && !!onToggleSelect
  const longPress = useRef<number | undefined>(undefined)

  const click = (ev: React.MouseEvent) => {
    // ⌘/Strg-Klick schaltet um, ⇧-Klick wählt einen Bereich (wie am Desktop)
    if (selectable && (selecting || ev.metaKey || ev.ctrlKey || ev.shiftKey)) onToggleSelect!(entry, ev.shiftKey)
    else onOpen(entry)
  }

  return (
    <Box
      // für die Rahmenauswahl mit der Maus (MarqueeArea)
      data-select-path={selectable ? entry.path : undefined}
      sx={{
        position: 'relative',
        userSelect: 'none',
        // Kacheln außerhalb des Sichtbereichs werden nicht gerendert
        contentVisibility: 'auto',
        containIntrinsicSize: compact ? 'auto 150px' : 'auto 220px',
        '&:hover .tile-hover, & .tile-hover:focus-visible': { opacity: 1 },
      }}
    >
      <ButtonBase
        onClick={click}
        onContextMenu={(ev) => {
          if (!onMenu || isSpace) return
          ev.preventDefault()
          onMenu(entry, ev.currentTarget)
        }}
        // Lange drücken (Handy) startet die Auswahl
        onTouchStart={() => {
          if (!selectable) return
          longPress.current = window.setTimeout(() => {
            longPress.current = undefined
            onToggleSelect!(entry)
          }, 500)
        }}
        onTouchEnd={(ev) => {
          if (longPress.current === undefined && selectable) ev.preventDefault()
          clearTimeout(longPress.current)
        }}
        onTouchMove={() => clearTimeout(longPress.current)}
        focusRipple
        sx={{ display: 'block', width: '100%', textAlign: 'left', borderRadius: compact ? 1.5 : 3 }}
      >
        <Box
          sx={{
            aspectRatio: '1',
            borderRadius: compact ? 1.5 : 3,
            overflow: 'hidden',
            bgcolor: 'action.hover',
            display: 'grid',
            placeItems: 'center',
            position: 'relative',
            transform: isSelected ? 'scale(.88)' : 'none',
            transition: 'transform .12s',
          }}
        >
          {(media || pageThumb) && !thumbFailed ? (
            <Box
              component="img"
              src={(source ?? userSource).thumb(entry)}
              alt={entry.name}
              loading="lazy"
              decoding="async"
              draggable={false}
              onError={() => setThumbFailed(true)}
              sx={{ width: '100%', height: '100%', objectFit: 'cover', display: 'block' }}
            />
          ) : (
            <FallbackIcon entry={entry} space={isSpace ? entry.name : undefined} />
          )}
          {pageThumb && !thumbFailed && (
            <Box
              sx={{
                position: 'absolute',
                left: 6,
                bottom: 6,
                px: 0.75,
                borderRadius: 1,
                bgcolor: 'rgba(0,0,0,.55)',
                color: 'common.white',
                fontSize: 11,
                fontWeight: 700,
                letterSpacing: 0.5,
              }}
            >
              {entry.name.split('.').pop()?.toUpperCase()}
            </Box>
          )}
          {entry.kind === 'video' && (
            <PlayCircleFilledRoundedIcon
              sx={{
                position: 'absolute',
                fontSize: compact ? 34 : 44,
                color: 'common.white',
                opacity: 0.9,
                filter: 'drop-shadow(0 1px 4px rgba(0,0,0,.5))',
              }}
            />
          )}
          {entry.live && <LiveBadge />}
          {entry.favorite && (
            <FavoriteRoundedIcon
              sx={{
                position: 'absolute',
                right: 6,
                bottom: 6,
                fontSize: 18,
                color: 'common.white',
                filter: 'drop-shadow(0 1px 3px rgba(0,0,0,.6))',
              }}
            />
          )}
        </Box>
        {!compact && (
          <Box sx={{ px: 0.5, pt: 0.75, pb: 0.5, minWidth: 0 }}>
            <Typography variant="body2" noWrap title={entry.name} sx={{ fontWeight: 500 }}>
              {entry.name}
            </Typography>
            <Typography variant="caption" color="text.secondary" noWrap component="div">
              {isSpace
                ? entry.name === 'Privat'
                  ? 'Nur für dich'
                  : 'Für alle Benutzer'
                : entry.kind === 'dir'
                  ? 'Ordner'
                  : formatSize(entry.size)}
            </Typography>
          </Box>
        )}
      </ButtonBase>

      {selectable && (
        <IconButton
          className={selecting ? undefined : 'tile-hover'}
          size="small"
          aria-label={isSelected ? 'Auswahl aufheben' : 'Auswählen'}
          onClick={(ev) => onToggleSelect!(entry, ev.shiftKey)}
          sx={{
            position: 'absolute',
            top: 4,
            left: 4,
            p: 0.25,
            opacity: selecting ? 1 : { xs: 0, md: 0 },
            transition: 'opacity .15s',
            color: isSelected ? 'primary.main' : 'common.white',
            filter: 'drop-shadow(0 1px 3px rgba(0,0,0,.5))',
            '&:hover': { bgcolor: 'transparent' },
          }}
        >
          {isSelected ? (
            <CheckCircleRoundedIcon sx={{ bgcolor: 'common.white', borderRadius: '50%' }} />
          ) : (
            <RadioButtonUncheckedRoundedIcon />
          )}
        </IconButton>
      )}
      {onMenu && !isSpace && !selecting && (
        <IconButton
          className="tile-hover"
          size="small"
          aria-label={`Aktionen für ${entry.name}`}
          onClick={(ev) => onMenu(entry, ev.currentTarget)}
          sx={{
            position: 'absolute',
            top: 6,
            right: 6,
            opacity: { xs: compact ? 0 : 1, md: 0 },
            transition: 'opacity .15s',
            bgcolor: 'rgba(0,0,0,.45)',
            color: 'common.white',
            '&:hover': { bgcolor: 'rgba(0,0,0,.65)' },
          }}
        >
          <MoreVertIcon fontSize="small" />
        </IconButton>
      )}
    </Box>
  )
})

function FallbackIcon({ entry, space }: { entry: Entry; space?: string }) {
  if (space === 'Privat') return <PersonRoundedIcon color="primary" sx={{ fontSize: 72 }} />
  if (space === 'Gemeinsam') return <GroupsRoundedIcon color="primary" sx={{ fontSize: 72 }} />
  if (entry.kind === 'dir') return <FolderRoundedIcon color="primary" sx={{ fontSize: 72 }} />
  return (
    <Box sx={{ '& svg': { fontSize: 64 } }}>
      <FileTypeIcon entry={entry} />
    </Box>
  )
}

/** Farbiges Symbol je Dateityp (wie in Google Drive) */
export function FileTypeIcon({ entry }: { entry: Entry }) {
  const ext = entry.name.split('.').pop()?.toLowerCase() ?? ''
  // Art aus der Endung, falls der Server sie nicht mitliefert (z. B. Dateien im Archiv)
  const is = (list: string) => list.split(' ').includes(ext)
  if (entry.kind === 'video' || is('mp4 mov m4v webm mkv avi 3gp')) return <MovieOutlinedIcon sx={{ color: 'text.secondary' }} />
  if (entry.kind === 'image' || is('jpg jpeg png gif webp heic heif tif tiff bmp'))
    return <ImageOutlinedIcon sx={{ color: 'text.secondary' }} />
  if (ext === 'pdf') return <PictureAsPdfRoundedIcon sx={{ color: '#e5483f' }} />
  if (['doc', 'docx', 'odt', 'rtf', 'pages'].includes(ext)) return <DescriptionRoundedIcon sx={{ color: '#3f7ee8' }} />
  if (['xls', 'xlsx', 'ods', 'csv', 'tsv', 'numbers'].includes(ext)) return <TableChartRoundedIcon sx={{ color: '#21a366' }} />
  if (['ppt', 'pptx', 'odp', 'key'].includes(ext)) return <SlideshowRoundedIcon sx={{ color: '#f0832d' }} />
  if (entry.doc === 'audio' || is('mp3 m4a aac wav flac ogg opus')) return <AudiotrackRoundedIcon sx={{ color: '#a35ee8' }} />
  if (entry.doc === 'text' || is('txt md log json xml yaml yml')) return <ArticleRoundedIcon sx={{ color: 'text.secondary' }} />
  if (['zip', 'rar', '7z', 'tar', 'gz'].includes(ext)) return <FolderZipRoundedIcon sx={{ color: '#c9a227' }} />
  return <InsertDriveFileOutlinedIcon sx={{ color: 'text.secondary' }} />
}

export function LiveBadge() {
  return (
    <Box
      sx={{
        position: 'absolute',
        left: 6,
        bottom: 6,
        display: 'flex',
        alignItems: 'center',
        gap: 0.5,
        px: 0.75,
        py: 0.25,
        borderRadius: 1,
        bgcolor: 'rgba(0,0,0,.45)',
        color: 'common.white',
        fontSize: 11,
        fontWeight: 700,
        letterSpacing: 0.5,
        backdropFilter: 'blur(4px)',
      }}
    >
      <AdjustRoundedIcon sx={{ fontSize: 14 }} />
      LIVE
    </Box>
  )
}
