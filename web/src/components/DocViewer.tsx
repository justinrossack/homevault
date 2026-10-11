import { useEffect, useMemo, useState } from 'react'
import { Box, Button, CircularProgress, IconButton, Paper, Stack, Table, TableBody, TableCell, TableRow, Typography } from '@mui/material'
import DownloadRoundedIcon from '@mui/icons-material/DownloadRounded'
import OpenInNewRoundedIcon from '@mui/icons-material/OpenInNewRounded'
import AudiotrackRoundedIcon from '@mui/icons-material/AudiotrackRounded'
import FolderZipRoundedIcon from '@mui/icons-material/FolderZipRounded'
import FolderRoundedIcon from '@mui/icons-material/FolderRounded'
import ChevronRightRoundedIcon from '@mui/icons-material/ChevronRightRounded'
import { api, formatSize, type ArchiveItem, type Entry } from '../api'
import { FileTypeIcon } from './FileGrid'

/** Größer als das wird Text nicht im Browser angezeigt */
const MAX_TEXT = 2 * 1024 * 1024
const MAX_CSV_ROWS = 1000

const ext = (name: string) => name.split('.').pop()?.toUpperCase() ?? ''

/**
 * Vorschau für Nicht-Medien: PDF und Office (als PDF), Text, Audio. Was nicht
 * geht, bekommt einen klaren Hinweis mit Download – wie bei Google Drive.
 */
export default function DocViewer({ item }: { item: Entry }) {
  switch (item.doc) {
    case 'pdf':
    case 'office':
      return <PdfView item={item} />
    case 'text':
      return <TextView item={item} />
    case 'audio':
      return <AudioView item={item} />
    case 'archive':
      return <ArchiveView item={item} />
    default:
      return <Unsupported item={item} />
  }
}

function Frame({ children }: { children: React.ReactNode }) {
  // unter der Titelleiste der Lightbox
  return <Box sx={{ position: 'absolute', inset: 0, top: 56, display: 'flex', flexDirection: 'column' }}>{children}</Box>
}

export function Unsupported({ item, reason }: { item: Entry; reason?: string }) {
  return (
    <Stack
      sx={{
        position: 'absolute',
        inset: 0,
        alignItems: 'center',
        justifyContent: 'center',
        gap: 2,
        p: 3,
        textAlign: 'center',
        color: 'common.white',
      }}
    >
      <Box sx={{ '& svg': { fontSize: 96 } }}>
        <FileTypeIcon entry={item} />
      </Box>
      <Typography variant="h6">{item.name}</Typography>
      <Typography sx={{ opacity: 0.8, maxWidth: 420 }}>
        {reason ?? `Für diesen Dateityp (${ext(item.name) || 'unbekannt'}) ist keine Vorschau verfügbar.`}
      </Typography>
      <Typography variant="body2" sx={{ opacity: 0.6 }}>
        {formatSize(item.size)}
      </Typography>
      <Button variant="contained" startIcon={<DownloadRoundedIcon />} href={api.downloadUrl(item.path)}>
        Herunterladen
      </Button>
    </Stack>
  )
}

function PdfView({ item }: { item: Entry }) {
  const url = api.docUrl(item)
  const [state, setState] = useState<'checking' | 'ready' | { error: string }>('checking')

  // Office-Dateien wandelt der Server erst um; vorher prüfen, ob das klappt
  useEffect(() => {
    let stop = false
    setState('checking')
    fetch(url, { headers: { Range: 'bytes=0-0' } })
      .then(async (r) => {
        if (stop) return
        if (r.ok) setState('ready')
        else setState({ error: (await r.json().catch(() => ({}))).error ?? 'Vorschau nicht möglich' })
      })
      .catch(() => !stop && setState({ error: 'Server nicht erreichbar' }))
    return () => {
      stop = true
    }
  }, [url])

  if (typeof state === 'object') return <Unsupported item={item} reason={`${state.error}.`} />
  if (state === 'checking') {
    return (
      <Stack sx={{ position: 'absolute', inset: 0, alignItems: 'center', justifyContent: 'center', gap: 2, color: 'common.white' }}>
        <CircularProgress color="inherit" />
        <Typography>{item.doc === 'office' ? 'Dokument wird für die Vorschau vorbereitet …' : 'Lade …'}</Typography>
      </Stack>
    )
  }
  return (
    <Frame>
      <Box component="iframe" src={url} title={item.name} sx={{ flex: 1, border: 0, bgcolor: '#525659', width: '100%' }} />
      {/* iPhone/iPad zeigen PDFs im Rahmen nur teilweise: dann im eigenen Tab */}
      <Stack direction="row" sx={{ justifyContent: 'center', py: 1, gap: 1, display: { xs: 'flex', md: 'none' } }}>
        <Button size="small" variant="contained" color="inherit" startIcon={<OpenInNewRoundedIcon />} href={url} target="_blank">
          Im neuen Tab öffnen
        </Button>
      </Stack>
    </Frame>
  )
}

/** CSV/TSV mit Anführungszeichen korrekt in Zeilen und Spalten zerlegen */
function parseCsv(text: string, sep: string): string[][] {
  const rows: string[][] = []
  let row: string[] = []
  let cell = ''
  let quoted = false
  for (let i = 0; i < text.length && rows.length < MAX_CSV_ROWS; i++) {
    const c = text[i]
    if (quoted) {
      if (c === '"' && text[i + 1] === '"') {
        cell += '"'
        i++
      } else if (c === '"') quoted = false
      else cell += c
    } else if (c === '"') quoted = true
    else if (c === sep) {
      row.push(cell)
      cell = ''
    } else if (c === '\n' || c === '\r') {
      if (c === '\r' && text[i + 1] === '\n') i++
      row.push(cell)
      rows.push(row)
      row = []
      cell = ''
    } else cell += c
  }
  if (cell || row.length) rows.push([...row, cell])
  return rows
}

function TextView({ item }: { item: Entry }) {
  const [text, setText] = useState<string | null>(null)
  const [error, setError] = useState('')
  const lower = item.name.toLowerCase()
  const sep = lower.endsWith('.csv')
    ? text && text.split('\n')[0].split(';').length > text.split('\n')[0].split(',').length
      ? ';'
      : ','
    : lower.endsWith('.tsv')
      ? '\t'
      : null

  useEffect(() => {
    if (item.size > MAX_TEXT) return
    let stop = false
    fetch(api.fileUrl(item.path))
      .then((r) => (r.ok ? r.text() : Promise.reject(new Error('Datei konnte nicht geladen werden'))))
      .then((t) => !stop && setText(t))
      .catch((e) => !stop && setError((e as Error).message))
    return () => {
      stop = true
    }
  }, [item])

  const rows = useMemo(() => (text !== null && sep ? parseCsv(text, sep) : null), [text, sep])

  if (item.size > MAX_TEXT) return <Unsupported item={item} reason="Die Datei ist zu groß für die Vorschau." />
  if (error) return <Unsupported item={item} reason={error} />
  if (text === null) {
    return (
      <Stack sx={{ position: 'absolute', inset: 0, alignItems: 'center', justifyContent: 'center', color: 'common.white' }}>
        <CircularProgress color="inherit" />
      </Stack>
    )
  }
  return (
    <Frame>
      <Box sx={{ flex: 1, overflow: 'auto', p: { xs: 1, sm: 3 } }}>
        <Paper sx={{ maxWidth: rows ? 'none' : 960, mx: 'auto', p: rows ? 0 : { xs: 2, sm: 3 }, overflow: 'auto' }}>
          {rows ? (
            <Table size="small" stickyHeader>
              <TableBody>
                {rows.map((r, i) => (
                  <TableRow key={i} sx={i === 0 ? { '& td': { fontWeight: 700, bgcolor: 'action.hover' } } : undefined}>
                    {r.map((c, j) => (
                      <TableCell key={j} sx={{ whiteSpace: 'nowrap', maxWidth: 360, overflow: 'hidden', textOverflow: 'ellipsis' }}>
                        {c}
                      </TableCell>
                    ))}
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          ) : (
            <Box
              component="pre"
              sx={{
                m: 0,
                fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace',
                fontSize: 13,
                lineHeight: 1.55,
                whiteSpace: 'pre-wrap',
                wordBreak: 'break-word',
              }}
            >
              {text}
            </Box>
          )}
        </Paper>
      </Box>
    </Frame>
  )
}

function AudioView({ item }: { item: Entry }) {
  return (
    <Stack sx={{ position: 'absolute', inset: 0, alignItems: 'center', justifyContent: 'center', gap: 3, p: 3, color: 'common.white' }}>
      <Box sx={{ width: 160, height: 160, borderRadius: 4, bgcolor: 'rgba(255,255,255,.08)', display: 'grid', placeItems: 'center' }}>
        <AudiotrackRoundedIcon sx={{ fontSize: 80, opacity: 0.8 }} />
      </Box>
      <Typography variant="h6" sx={{ textAlign: 'center' }}>
        {item.name}
      </Typography>
      <Box component="audio" src={api.fileUrl(item.path)} controls autoPlay sx={{ width: 'min(480px, 100%)' }} />
    </Stack>
  )
}

interface TreeNode {
  name: string
  path: string
  dir: boolean
  size: number
  encrypted?: boolean
  children: Map<string, TreeNode>
}

/** Flache Liste aus dem Archiv in einen Ordnerbaum umbauen (Ordner zuerst). */
function buildTree(items: ArchiveItem[]): TreeNode {
  const root: TreeNode = { name: '', path: '', dir: true, size: 0, children: new Map() }
  for (const it of items) {
    const parts = it.path.replace(/\/$/, '').split('/')
    let node = root
    parts.forEach((part, i) => {
      const last = i === parts.length - 1
      let child = node.children.get(part)
      if (!child) {
        child = { name: part, path: parts.slice(0, i + 1).join('/'), dir: !last || it.dir, size: 0, children: new Map() }
        node.children.set(part, child)
      }
      if (last && !it.dir) {
        child.size = it.size
        child.encrypted = it.encrypted
      }
      node = child
    })
  }
  // Ordnergrößen aufsummieren
  const sum = (n: TreeNode): number => (n.dir ? (n.size = [...n.children.values()].reduce((a, c) => a + sum(c), 0)) : n.size)
  sum(root)
  return root
}

const sorted = (n: TreeNode) =>
  [...n.children.values()].sort((a, b) => Number(b.dir) - Number(a.dir) || a.name.localeCompare(b.name, 'de', { numeric: true }))

function ArchiveView({ item }: { item: Entry }) {
  const [data, setData] = useState<{ tree: TreeNode; files: number; total: number; truncated: boolean } | null>(null)
  const [error, setError] = useState('')

  useEffect(() => {
    let stop = false
    api
      .archive(item.path)
      .then((r) => !stop && setData({ tree: buildTree(r.entries), files: r.files, total: r.total_size, truncated: r.truncated }))
      .catch((e) => !stop && setError((e as Error).message))
    return () => {
      stop = true
    }
  }, [item])

  if (error) return <Unsupported item={item} reason={error} />
  if (!data) {
    return (
      <Stack sx={{ position: 'absolute', inset: 0, alignItems: 'center', justifyContent: 'center', color: 'common.white' }}>
        <CircularProgress color="inherit" />
      </Stack>
    )
  }
  return (
    <Frame>
      <Box sx={{ flex: 1, overflow: 'auto', p: { xs: 1, sm: 3 } }}>
        <Paper sx={{ maxWidth: 900, mx: 'auto', overflow: 'hidden' }}>
          <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5, px: 2, py: 1.5, borderBottom: 1, borderColor: 'divider' }}>
            <FolderZipRoundedIcon sx={{ color: '#c9a227' }} />
            <Box sx={{ flex: 1 }}>
              <Typography sx={{ fontWeight: 600 }}>{item.name}</Typography>
              <Typography variant="body2" color="text.secondary">
                {data.files} {data.files === 1 ? 'Datei' : 'Dateien'} · entpackt {formatSize(data.total)} · gepackt {formatSize(item.size)}
              </Typography>
            </Box>
            <Button size="small" startIcon={<DownloadRoundedIcon />} href={api.downloadUrl(item.path)}>
              Ganzes Archiv
            </Button>
          </Stack>
          {data.truncated && (
            <Typography variant="body2" color="warning.main" sx={{ px: 2, pt: 1 }}>
              Das Archiv ist sehr groß, es werden nur die ersten 10.000 Einträge angezeigt.
            </Typography>
          )}
          <Box sx={{ py: 0.5 }}>
            {sorted(data.tree).map((n) => (
              <TreeRow key={n.path} node={n} depth={0} archive={item} startOpen={data.tree.children.size === 1} />
            ))}
          </Box>
        </Paper>
      </Box>
    </Frame>
  )
}

function TreeRow({ node, depth, archive, startOpen }: { node: TreeNode; depth: number; archive: Entry; startOpen?: boolean }) {
  const [open, setOpen] = useState(!!startOpen)
  const fake: Entry = { name: node.name, path: node.path, kind: 'other', size: node.size, modified: 0 }
  return (
    <>
      <Stack
        direction="row"
        onClick={node.dir ? () => setOpen((o) => !o) : undefined}
        sx={{
          alignItems: 'center',
          gap: 1,
          pl: 1.5 + depth * 2.5,
          pr: 1,
          minHeight: 40,
          cursor: node.dir ? 'pointer' : 'default',
          '&:hover': { bgcolor: 'action.hover' },
        }}
      >
        <Box sx={{ width: 20, display: 'flex', color: 'text.secondary' }}>
          {node.dir && (
            <ChevronRightRoundedIcon fontSize="small" sx={{ transform: open ? 'rotate(90deg)' : 'none', transition: 'transform .15s' }} />
          )}
        </Box>
        <Box sx={{ display: 'flex', '& svg': { fontSize: 22 } }}>
          {node.dir ? <FolderRoundedIcon color="primary" /> : <FileTypeIcon entry={fake} />}
        </Box>
        <Typography variant="body2" noWrap sx={{ flex: 1, minWidth: 0 }}>
          {node.name}
          {node.encrypted && (
            <Typography component="span" variant="caption" color="warning.main" sx={{ ml: 1 }}>
              verschlüsselt
            </Typography>
          )}
        </Typography>
        <Typography variant="body2" color="text.secondary" sx={{ fontVariantNumeric: 'tabular-nums', whiteSpace: 'nowrap' }}>
          {node.dir ? `${node.children.size} ${node.children.size === 1 ? 'Eintrag' : 'Einträge'}` : formatSize(node.size)}
        </Typography>
        <Box sx={{ width: 40 }}>
          {!node.dir && !node.encrypted && (
            <IconButton size="small" href={api.archiveFileUrl(archive.path, node.path)} aria-label={`${node.name} herunterladen`}>
              <DownloadRoundedIcon fontSize="small" />
            </IconButton>
          )}
        </Box>
      </Stack>
      {node.dir && open && sorted(node).map((c) => <TreeRow key={c.path} node={c} depth={depth + 1} archive={archive} />)}
    </>
  )
}
