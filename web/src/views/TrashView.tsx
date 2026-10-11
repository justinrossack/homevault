import { useCallback, useEffect, useState } from 'react'
import {
  Alert,
  Avatar,
  Box,
  Button,
  Checkbox,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  IconButton,
  List,
  ListItem,
  ListItemAvatar,
  ListItemText,
  Stack,
  Tooltip,
} from '@mui/material'
import DeleteOutlineRoundedIcon from '@mui/icons-material/DeleteOutlineRounded'
import DeleteForeverRoundedIcon from '@mui/icons-material/DeleteForeverRounded'
import RestoreRoundedIcon from '@mui/icons-material/RestoreRounded'
import FolderRoundedIcon from '@mui/icons-material/FolderRounded'
import InsertDriveFileOutlinedIcon from '@mui/icons-material/InsertDriveFileOutlined'
import { api, formatSize, type TrashItem } from '../api'
import { useShell } from '../shell'
import { Empty, Loading, ViewHeader } from './common'

const day = (t: number) => new Date(t * 1000).toLocaleDateString('de-DE', { day: 'numeric', month: 'short', year: 'numeric' })

export default function TrashView() {
  const shell = useShell()
  const [items, setItems] = useState<TrashItem[] | null>(null)
  const [days, setDays] = useState(30)
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [confirm, setConfirm] = useState<'delete' | 'empty' | null>(null)
  const [error, setError] = useState('')

  const load = useCallback(() => {
    api
      .trash()
      .then((r) => {
        setItems(r.items)
        setDays(r.days)
        setSelected(new Set())
      })
      .catch((e) => setError((e as Error).message))
  }, [])
  useEffect(load, [load])

  async function act(fn: () => Promise<unknown>, msg: string) {
    try {
      await fn()
      shell.toast(msg)
    } catch (e) {
      shell.toast((e as Error).message)
    }
    setConfirm(null)
    load()
  }

  const ids = [...selected]
  const restore = (list: number[]) =>
    act(() => api.restoreTrash(list), list.length === 1 ? 'Wiederhergestellt' : `${list.length} wiederhergestellt`)

  return (
    <>
      <ViewHeader
        title="Papierkorb"
        subtitle={`Gelöschtes bleibt ${days} Tage hier und wird dann endgültig entfernt.`}
        actions={
          items && items.length > 0 ? (
            ids.length > 0 ? (
              <Stack direction="row" spacing={1}>
                <Button variant="contained" startIcon={<RestoreRoundedIcon />} onClick={() => restore(ids)}>
                  {ids.length} wiederherstellen
                </Button>
                <Tooltip title="Endgültig löschen">
                  <IconButton color="error" onClick={() => setConfirm('delete')}>
                    <DeleteForeverRoundedIcon />
                  </IconButton>
                </Tooltip>
              </Stack>
            ) : (
              <Button color="error" startIcon={<DeleteForeverRoundedIcon />} onClick={() => setConfirm('empty')}>
                Papierkorb leeren
              </Button>
            )
          ) : undefined
        }
      />
      {error && <Alert severity="error">{error}</Alert>}
      {items === null ? (
        !error && <Loading />
      ) : items.length === 0 ? (
        <Empty icon={<DeleteOutlineRoundedIcon />}>Der Papierkorb ist leer.</Empty>
      ) : (
        <List>
          {items.map((it) => (
            <ListItem
              key={it.id}
              disableGutters
              secondaryAction={
                <Tooltip title="Wiederherstellen">
                  <IconButton onClick={() => restore([it.id])}>
                    <RestoreRoundedIcon />
                  </IconButton>
                </Tooltip>
              }
              sx={{ borderRadius: 2, '&:hover': { bgcolor: 'action.hover' } }}
            >
              <Checkbox
                checked={selected.has(it.id)}
                onChange={() =>
                  setSelected((s) => {
                    const n = new Set(s)
                    if (n.has(it.id)) n.delete(it.id)
                    else n.add(it.id)
                    return n
                  })
                }
              />
              <ListItemAvatar>
                <Avatar
                  variant="rounded"
                  src={it.kind === 'image' || it.kind === 'video' ? api.trashThumbUrl(it.id) : undefined}
                  sx={{ width: 48, height: 48, mr: 1.5, bgcolor: 'action.hover', color: 'text.secondary' }}
                >
                  {it.kind === 'dir' ? <FolderRoundedIcon color="primary" /> : <InsertDriveFileOutlinedIcon />}
                </Avatar>
              </ListItemAvatar>
              <ListItemText
                primary={it.name}
                secondary={
                  <Box component="span">
                    aus „{it.location || it.space}“ · {formatSize(it.size)} · gelöscht am {day(it.deleted_at)} von {it.deleted_by}
                    {' · '}endgültig am {day(it.expires_at)}
                  </Box>
                }
                slotProps={{ primary: { noWrap: true }, secondary: { noWrap: true } }}
                sx={{ pr: 6 }}
              />
            </ListItem>
          ))}
        </List>
      )}

      <Dialog open={!!confirm} onClose={() => setConfirm(null)}>
        <DialogTitle>{confirm === 'empty' ? 'Papierkorb leeren?' : `${ids.length} Elemente endgültig löschen?`}</DialogTitle>
        <DialogContent>
          <DialogContentText>
            {confirm === 'empty'
              ? 'Alle Elemente, die du hier siehst, werden endgültig gelöscht. Das lässt sich nicht rückgängig machen.'
              : 'Diese Elemente werden endgültig gelöscht. Das lässt sich nicht rückgängig machen.'}
          </DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setConfirm(null)}>Abbrechen</Button>
          <Button
            color="error"
            variant="contained"
            onClick={() =>
              confirm === 'empty'
                ? act(() => api.emptyTrash(), 'Papierkorb geleert')
                : act(() => api.deleteTrash(ids), 'Endgültig gelöscht')
            }
          >
            Endgültig löschen
          </Button>
        </DialogActions>
      </Dialog>
    </>
  )
}
