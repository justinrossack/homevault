import { useCallback, useEffect, useRef, useState } from 'react'
import { Box, Button, ButtonBase, CircularProgress, Dialog, IconButton, Stack, Typography } from '@mui/material'
import CloseRoundedIcon from '@mui/icons-material/CloseRounded'
import ChevronLeftRoundedIcon from '@mui/icons-material/ChevronLeftRounded'
import ChevronRightRoundedIcon from '@mui/icons-material/ChevronRightRounded'
import DownloadRoundedIcon from '@mui/icons-material/DownloadRounded'
import AdjustRoundedIcon from '@mui/icons-material/AdjustRounded'
import FavoriteRoundedIcon from '@mui/icons-material/FavoriteRounded'
import FavoriteBorderRoundedIcon from '@mui/icons-material/FavoriteBorderRounded'
import PhotoAlbumOutlinedIcon from '@mui/icons-material/PhotoAlbumOutlined'
import DeleteOutlineRoundedIcon from '@mui/icons-material/DeleteOutlineRounded'
import ShareRoundedIcon from '@mui/icons-material/ShareRounded'
import { formatTaken, isMedia, userSource, type Entry, type MediaSource, type VideoStatus } from '../api'
import DocViewer from './DocViewer'

interface Props {
  items: Entry[]
  index: number | null
  onIndex: (i: number | null) => void
  onToggleFavorite?: (e: Entry) => void
  onAddToAlbum?: (e: Entry) => void
  onDelete?: (e: Entry) => void
  onShare?: (e: Entry) => void
  /** Woher die Bilder kommen (Standard: eigene Dateien) */
  source?: MediaSource
}

const navBtn = {
  position: 'absolute',
  top: '50%',
  transform: 'translateY(-50%)',
  color: 'common.white',
  bgcolor: 'rgba(255,255,255,.08)',
  '&:hover': { bgcolor: 'rgba(255,255,255,.18)' },
} as const

const fill = { position: 'absolute', inset: 0, width: '100%', height: '100%', objectFit: 'contain' } as const

export default function Lightbox({ items, index, onIndex, onToggleFavorite, onAddToAlbum, onDelete, onShare, source = userSource }: Props) {
  const item = index !== null ? items[index] : undefined
  const go = useCallback(
    (d: number) => index !== null && onIndex((index + d + items.length) % items.length),
    [index, items.length, onIndex],
  )

  useEffect(() => {
    if (index === null) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'ArrowLeft') go(-1)
      if (e.key === 'ArrowRight') go(1)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [index, go])

  // nächstes Bild vorladen, damit das Weiterblättern sofort geht
  useEffect(() => {
    if (index === null || items.length < 2) return
    const next = items[(index + 1) % items.length]
    if (next.kind === 'image') new Image().src = source.view(next)
  }, [index, items, source])
  const download = item ? source.download(item) : null

  return (
    <Dialog fullScreen open={!!item} onClose={() => onIndex(null)} slotProps={{ paper: { sx: { bgcolor: '#000' } } }}>
      {item && (
        <Box sx={{ position: 'relative', height: '100dvh', overflow: 'hidden' }}>
          {!isMedia(item) ? (
            <DocViewer key={item.path} item={item} />
          ) : item.kind === 'video' ? (
            <VideoPlayer key={item.path} item={item} source={source} />
          ) : item.live ? (
            <LivePhoto key={item.path} item={item} source={source} />
          ) : (
            <Box component="img" key={item.path} src={source.view(item)} alt={item.name} sx={{ ...fill, userSelect: 'none' }} />
          )}

          <Stack
            direction="row"
            sx={{
              position: 'absolute',
              top: 0,
              left: 0,
              right: 0,
              p: 1,
              alignItems: 'center',
              gap: 1,
              color: 'common.white',
              background: 'linear-gradient(rgba(0,0,0,.6), transparent)',
              pointerEvents: 'none',
              '& > *': { pointerEvents: 'auto' },
            }}
          >
            <Typography noWrap sx={{ flex: 1, pl: 1 }}>
              {item.name}
              <Typography component="span" sx={{ opacity: 0.6, ml: 1.5 }} variant="body2">
                {item.taken
                  ? formatTaken(item.taken, { day: 'numeric', month: 'long', year: 'numeric', hour: '2-digit', minute: '2-digit' })
                  : `${index! + 1} / ${items.length}`}
              </Typography>
            </Typography>
            {onToggleFavorite && isMedia(item) && (
              <IconButton onClick={() => onToggleFavorite(item)} aria-label="Favorit" sx={{ color: 'inherit' }}>
                {item.favorite ? <FavoriteRoundedIcon sx={{ color: '#ff5a7a' }} /> : <FavoriteBorderRoundedIcon />}
              </IconButton>
            )}
            {onAddToAlbum && isMedia(item) && (
              <IconButton onClick={() => onAddToAlbum(item)} aria-label="Zu Album hinzufügen" sx={{ color: 'inherit' }}>
                <PhotoAlbumOutlinedIcon />
              </IconButton>
            )}
            {onShare && isMedia(item) && (
              <IconButton onClick={() => onShare(item)} aria-label="Teilen" sx={{ color: 'inherit' }}>
                <ShareRoundedIcon />
              </IconButton>
            )}
            {onDelete && (
              <IconButton onClick={() => onDelete(item)} aria-label="Löschen" sx={{ color: 'inherit' }}>
                <DeleteOutlineRoundedIcon />
              </IconButton>
            )}
            {download && (
              <IconButton component="a" href={download} aria-label="Herunterladen" sx={{ color: 'inherit' }}>
                <DownloadRoundedIcon />
              </IconButton>
            )}
            <IconButton onClick={() => onIndex(null)} aria-label="Schließen" sx={{ color: 'inherit' }}>
              <CloseRoundedIcon />
            </IconButton>
          </Stack>

          {items.length > 1 && (
            <>
              <IconButton onClick={() => go(-1)} aria-label="Zurück" sx={{ ...navBtn, left: 12 }}>
                <ChevronLeftRoundedIcon fontSize="large" />
              </IconButton>
              <IconButton onClick={() => go(1)} aria-label="Weiter" sx={{ ...navBtn, right: 12 }}>
                <ChevronRightRoundedIcon fontSize="large" />
              </IconButton>
            </>
          )}
        </Box>
      )}
    </Dialog>
  )
}

/**
 * Live Photo wie auf dem iPhone: Beim Öffnen spielt die Bewegung einmal
 * stumm ab. Gedrückt halten (Foto) oder auf LIVE klicken spielt erneut, mit Ton.
 */
function LivePhoto({ item, source }: { item: Entry; source: MediaSource }) {
  const video = useRef<HTMLVideoElement>(null)
  const autoplayed = useRef(false)
  const [playing, setPlaying] = useState(false)

  const play = useCallback((withSound: boolean) => {
    const v = video.current
    if (!v) return
    v.muted = !withSound
    v.currentTime = 0
    v.play()
      .then(() => setPlaying(true))
      .catch(() => {
        // Abspielen mit Ton blockiert: stumm versuchen
        if (!v.muted) {
          v.muted = true
          v.play().then(
            () => setPlaying(true),
            () => {},
          )
        }
      })
  }, [])
  const stop = () => {
    video.current?.pause()
    setPlaying(false)
  }

  return (
    <>
      <Box component="img" src={source.view(item)} alt={item.name} sx={{ ...fill, userSelect: 'none' }} />
      <Box
        component="video"
        ref={video}
        src={source.live(item)}
        playsInline
        muted
        preload="auto"
        onCanPlay={() => {
          // nur beim ersten Laden automatisch abspielen
          if (!autoplayed.current) {
            autoplayed.current = true
            play(false)
          }
        }}
        onEnded={() => setPlaying(false)}
        onPointerDown={() => play(true)}
        onPointerUp={stop}
        onPointerLeave={() => playing && stop()}
        onContextMenu={(e: React.MouseEvent) => e.preventDefault()}
        sx={{ ...fill, opacity: playing ? 1 : 0, transition: 'opacity .25s', cursor: 'pointer', touchAction: 'none' }}
      />
      <ButtonBase
        onClick={() => play(true)}
        aria-label="Live Photo abspielen"
        sx={{
          position: 'absolute',
          left: 16,
          bottom: 16,
          display: 'flex',
          gap: 0.5,
          px: 1,
          py: 0.5,
          borderRadius: 1.5,
          color: 'common.white',
          fontSize: 12,
          fontWeight: 700,
          letterSpacing: 0.5,
          bgcolor: playing ? 'rgba(255,255,255,.3)' : 'rgba(0,0,0,.45)',
          backdropFilter: 'blur(4px)',
        }}
      >
        <AdjustRoundedIcon sx={{ fontSize: 16 }} />
        LIVE
      </ButtonBase>
    </>
  )
}

/** Spielt HEVC direkt ab, wenn der Browser es kann (Safari) */
const canPlayHevc = () => {
  const v = document.createElement('video')
  return v.canPlayType('video/mp4; codecs="hvc1.1.6.L93.B0"') !== '' || v.canPlayType('video/mp4; codecs="hev1.1.6.L93.B0"') !== ''
}

/**
 * Video abspielen: Original, wenn der Browser es kann, sonst die vom Server
 * umgewandelte H.264-Fassung. Ist die noch nicht fertig, wird der
 * Fortschritt angezeigt und regelmäßig nachgefragt.
 */
function VideoPlayer({ item, source }: { item: Entry; source: MediaSource }) {
  const [src, setSrc] = useState<string | null>(null)
  const [status, setStatus] = useState<VideoStatus | null>(null)

  useEffect(() => {
    let stop = false
    let timer: number | undefined
    const check = async () => {
      try {
        const s = await source.videoStatus(item)
        if (stop) return
        setStatus(s)
        if (s.mode === 'original') setSrc(source.file(item))
        else if (s.mode === 'web') setSrc(source.web(item))
        else if (s.codec === 'hevc' && canPlayHevc()) setSrc(source.file(item))
        else if (s.mode === 'pending') timer = window.setTimeout(check, 2000)
      } catch {
        // Status unbekannt: Original versuchen
        if (!stop) setSrc(source.file(item))
      }
    }
    check()
    return () => {
      stop = true
      clearTimeout(timer)
    }
  }, [item, source])

  if (src) return <Box component="video" src={src} controls autoPlay playsInline sx={fill} />

  const download = source.download(item)
  return (
    <Stack sx={{ ...fill, alignItems: 'center', justifyContent: 'center', gap: 2, color: 'common.white', px: 3, textAlign: 'center' }}>
      {status?.mode === 'failed' ? (
        <Typography>Dieses Video kann im Browser leider nicht abgespielt werden.</Typography>
      ) : (
        <>
          <CircularProgress
            color="inherit"
            variant={status?.progress ? 'determinate' : 'indeterminate'}
            value={(status?.progress ?? 0) * 100}
          />
          <Typography>
            Video wird für deinen Browser vorbereitet
            {status?.progress ? ` … ${Math.round(status.progress * 100)} %` : ' …'}
          </Typography>
          <Typography variant="body2" sx={{ opacity: 0.7 }}>
            Das passiert nur einmal, danach startet es sofort.
          </Typography>
        </>
      )}
      {download && (
        <Button variant="outlined" color="inherit" href={download} startIcon={<DownloadRoundedIcon />}>
          Original herunterladen
        </Button>
      )}
    </Stack>
  )
}
