export type Kind = 'dir' | 'image' | 'video' | 'other'

export interface Entry {
  name: string
  path: string
  kind: Kind
  size: number
  modified: number
  /** Browser kann das Format nicht anzeigen, Server liefert JPEG-Vorschau (HEIC, TIFF) */
  preview?: boolean
  /** Pfad des Videos, falls Live Photo */
  live?: string
  favorite?: boolean
  /** Aufnahmezeit (Ortszeit als Unix-Sekunden), nur in Zeitleiste/Favoriten/Alben */
  taken?: number
  /** Index-ID (Teilen-Links) */
  id?: number
  /** Art der Dokument-Vorschau */
  doc?: 'pdf' | 'office' | 'text' | 'audio' | 'archive'
}

export interface User {
  name: string
  admin: boolean
}

export interface Me {
  authenticated: boolean
  user?: User | null
  setup_needed: boolean
  server_name: string
}

export interface Album {
  id: number
  name: string
  count: number
  created_at: number
  cover?: Entry | null
}

export interface IndexStatus {
  scanning: boolean
  items: number
  last_scan?: number | null
  last_duration_ms?: number | null
  last_error?: string | null
}

export interface BackupRun {
  started_at: number
  finished_at?: number | null
  status: 'running' | 'ok' | 'error' | 'cancelled'
  files: number
  bytes: number
  message?: string | null
}

export interface BackupTargetInput {
  name: string
  host: string
  port: number
  share: string
  folder: string
  username: string
  /** leer = beim Bearbeiten unverändert */
  password?: string
  include_shared: boolean
  include_private: boolean
  include_db: boolean
  schedule: 'daily' | 'weekly' | 'manual'
  time: string
  /** 0 = Montag */
  weekday: number
  keep_days: number
  enabled: boolean
  /** Wer seinen Privat-Ordner selbst hierher synchronisieren darf */
  user_sync: 'none' | 'all' | 'selected'
  /** bei 'selected' */
  sync_users: string[]
  /** Netzwerkadapter für Backup/Sync, '' = automatisch */
  iface: string
}

/** Netzwerkadapter des Servers */
export interface NetworkAdapter {
  name: string
  ipv4: string | null
  up: boolean
  speed_mbit: number | null
  mac: string | null
  kind: 'lan' | 'usb' | 'wlan'
}

export interface BackupTarget extends BackupTargetInput {
  id: number
  has_password: boolean
  location: string
  last_run?: BackupRun | null
  last_success?: number | null
  next_run?: number | null
}

export type SyncStatus = 'new' | 'pending' | 'running' | 'ok' | 'error' | 'paused'

export interface SyncRule {
  id: number
  target_id: number
  target_name: string
  /** \\host\freigabe\ziel */
  location: string
  source_space: 'Gemeinsam' | 'Privat'
  source_user: string | null
  source_sub: string
  /** z. B. „Gemeinsam/Fotos“ oder „Privat von anna/Fotos“ */
  source_label: string
  dest: string
  by_admin: boolean
  can_edit: boolean
  keep_days: number
  enabled: boolean
  status: SyncStatus
  pending_since: number | null
  last_run_at: number | null
  last_ok_at: number | null
  last_error: string | null
  /** übertragen im letzten Lauf */
  files: number
  bytes: number
}

/** NAS, das man als Sync-Ziel wählen darf */
export interface SyncTarget {
  id: number
  name: string
  location: string
  folder: string
}

export interface SyncRuleInput {
  target_id: number
  source: 'shared' | 'private'
  /** nur Admin: wessen Privat-Ordner */
  user?: string
  /** Unterordner der Quelle, '' = alles */
  sub?: string
  /** nur Admin: Ordner in der Freigabe */
  dest?: string
  keep_days?: number
  /** Zielordner ist nicht leer – trotzdem anlegen */
  confirm_nonempty?: boolean
}

export interface BackupItem {
  name: string
  path: string
  dir: boolean
  size?: number | null
  modified?: string
}

export interface RestoreResult {
  ok: boolean
  files?: number
  bytes?: number
  folder?: string | null
  message?: string
  finished_at: number
}

export interface BackupProgress {
  target_id: number
  part: string
  bytes: number
  total_bytes: number
  transfers: number
  total_transfers: number
  speed: number
  started_at: number
}

export interface ArchiveItem {
  path: string
  dir: boolean
  size: number
  compressed?: number
  encrypted?: boolean
}

export interface DiskInfo {
  path: string
  name: string
  size: number
  model: string
  serial?: string | null
  ssd: boolean
  transport?: string | null
  hotplug?: boolean | null
  in_pool: boolean
  usable: boolean
  reason?: string | null
  existing: string[]
  old_member?: boolean
  smart?: {
    passed?: boolean | null
    temperature?: number | null
    power_on_hours?: number | null
    reallocated?: number | null
    pending?: number | null
  } | null
}

export interface PoolDevice {
  devid: number
  size?: number | null
  used?: number | null
  path?: string | null
  missing: boolean
  errors?: number
}

export interface DisksStatus {
  available: boolean
  reachable?: boolean
  error?: string
  ready?: boolean
  maintenance?: boolean
  last_error?: string | null
  job?: { kind: string; message: string; started_at: number } | null
  disks?: DiskInfo[]
  pool?: {
    uuid: string
    profile: string
    data_profile?: string | null
    mounted: boolean
    degraded: boolean
    devices: PoolDevice[]
    size?: number
    used?: number
    free?: number
    last_scrub?: number | null
    scrub?: { running: boolean; status?: string | null; errors?: string | null }
    replace?: { running: boolean; progress?: number | null }
    balance?: { running: boolean; left?: number | null }
  } | null
}

export interface Settings {
  server_name: string
  trash_days: number
  auto_transcode: boolean
  inbox_enabled: boolean
}

export type InboxAiStatus =
  { state: 'unknown' } | { state: 'unreachable'; error: string } | { state: 'pulling'; done: number; total: number } | { state: 'ready' }

export interface InboxItem {
  id: number
  /** 'Gemeinsam' oder 'Privat/<benutzer>' */
  area: string
  original_name: string
  /** Pfad wie in der Dateiansicht, nach „Rückgängig“ im Ordner Eingang/Zurückgelegt */
  new_path: string | null
  kind: 'photo' | 'document' | 'other'
  status: 'done' | 'error' | 'undone' | 'skipped'
  message: string | null
  created_at: number
}

export interface InboxStatus {
  enabled: boolean
  /** null = keine KI eingerichtet, Dokumente bleiben liegen */
  ai: { model: string; status: InboxAiStatus } | null
  waiting: { name: string; area: string; document: boolean }[]
  /** gerade bearbeitete Datei (Pfad auf dem Server, z. B. „Gemeinsam/Eingang/scan.pdf“) */
  current: string | null
  items: InboxItem[]
}

export interface TrashItem {
  id: number
  space: string
  name: string
  kind: Kind
  size: number
  location: string
  deleted_by: string
  deleted_at: number
  expires_at: number
  live: boolean
}

export interface AreaUsage {
  bytes: number
  files: number
  photo_bytes: number
  video_bytes: number
}

export interface Storage {
  disk: { total: number; free: number }
  warning: boolean
  me: { used: number; quota: number | null }
  shared: AreaUsage
  last_scan: number | null
  users?: { name: string; quota: number | null; usage: AreaUsage }[]
  trash?: number
  cache?: number
}

export interface VideoStatus {
  mode: 'original' | 'web' | 'pending' | 'failed'
  codec?: string | null
  progress?: number | null
}

export interface ShareLink {
  id: number
  url: string
  kind: 'album' | 'file'
  title: string | null
  path?: string | null
  has_password: boolean
  allow_download: boolean
  expires_at: number | null
  expired: boolean
  views: number
  created_at: number
}

export interface PublicShare {
  needs_password: boolean
  server_name: string
  title?: string
  owner?: string
  kind?: 'album' | 'file'
  allow_download?: boolean
  expires_at?: number | null
  entries?: (Omit<Entry, 'live' | 'path'> & { id: number; live: boolean })[]
}

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message)
  }
}

/** Wird aufgerufen, sobald der Server 401 meldet (Session abgelaufen). */
let onUnauthorized = () => {}
export const setUnauthorizedHandler = (fn: () => void) => (onUnauthorized = fn)

async function request<T>(method: string, url: string, body?: unknown): Promise<T> {
  const res = await fetch(url, {
    method,
    headers: body ? { 'content-type': 'application/json' } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  }).catch(() => {
    // keine Antwort (Server weg, Netzwerk)
    throw new ApiError(0, 'Server nicht erreichbar')
  })
  const data = await res.json().catch(() => ({}))
  if (!res.ok) {
    if (res.status === 401 && !url.endsWith('/login')) onUnauthorized()
    throw new ApiError(res.status, data.error ?? res.statusText)
  }
  return data as T
}

const q = (path: string) => `path=${encodeURIComponent(path)}`
const v = (e: Entry) => `v=${e.modified}-${e.size}`

export const api = {
  me: () => request<Me>('GET', '/api/me'),
  setup: (body: { server_name: string; name: string; password: string }) => request('POST', '/api/setup', body),
  login: (username: string, password: string) => request('POST', '/api/login', { username, password }),
  logout: () => request('POST', '/api/logout'),
  changePassword: (current: string, next: string) => request('POST', '/api/me/password', { current, new: next }),

  // Verwaltung (Admin)
  network: () => request<{ adapters: NetworkAdapter[] }>('GET', '/api/network'),
  users: () => request<{ users: (User & { created_at: number; quota_bytes?: number | null })[] }>('GET', '/api/users'),
  createUser: (name: string, password: string, admin: boolean) => request('POST', '/api/users', { name, password, admin }),
  updateUser: (name: string, patch: { password?: string; admin?: boolean; quota_bytes?: number }) =>
    request('PATCH', `/api/users/${encodeURIComponent(name)}`, patch),
  deleteUser: (name: string) => request('DELETE', `/api/users/${encodeURIComponent(name)}`),
  settings: () => request<Settings>('GET', '/api/settings'),
  disks: () => request<DisksStatus>('GET', '/api/disks'),
  disksSummary: () => request<{ state: 'ok' | 'none' | 'degraded' | 'warning' | 'unreachable' }>('GET', '/api/disks/summary'),
  diskAction: (action: 'create' | 'add' | 'replace' | 'remove' | 'scrub', body: object = {}) =>
    request('POST', `/api/disks/${action}`, body),
  health: () => fetch('/api/health').then((r) => r.ok),
  updateSettings: (patch: Partial<Settings>) => request('PATCH', '/api/settings', patch),
  storage: () => request<Storage>('GET', '/api/storage'),
  trash: () => request<{ items: TrashItem[]; days: number }>('GET', '/api/trash'),
  restoreTrash: (ids: number[]) => request<{ restored: string[] }>('POST', '/api/trash/restore', { ids }),
  deleteTrash: (ids: number[]) => request('POST', '/api/trash/delete', { ids }),
  emptyTrash: () => request<{ deleted: number }>('POST', '/api/trash/empty'),
  trashThumbUrl: (id: number) => `/api/trash/${id}/thumb`,
  shares: (filter: { album_id?: number; path?: string } = {}) => {
    const p = new URLSearchParams()
    if (filter.album_id) p.set('album_id', String(filter.album_id))
    if (filter.path) p.set('path', filter.path)
    return request<{ shares: ShareLink[] }>('GET', `/api/shares?${p}`)
  },
  createShare: (body: { album_id?: number; path?: string; password?: string; expires_days?: number; allow_download: boolean }) =>
    request<{ token: string; url: string }>('POST', '/api/shares', body),
  deleteShare: (id: number) => request('DELETE', `/api/shares/${id}`),
  publicShare: (token: string) => request<PublicShare>('GET', `/api/s/${token}`),
  unlockShare: (token: string, password: string) => request('POST', `/api/s/${token}/unlock`, { password }),
  indexStatus: () =>
    request<{ index: IndexStatus; db_size: number; transcoder: { current: { name: string; progress: number } | null; queued: number } }>(
      'GET',
      '/api/index',
    ),
  rescan: () => request('POST', '/api/index/rescan'),
  backups: () =>
    request<{ targets: BackupTarget[]; running: BackupProgress | null; restore?: RestoreResult | null }>('GET', '/api/backups'),
  createBackup: (t: BackupTargetInput) => request<{ id: number }>('POST', '/api/backups', t),
  updateBackup: (id: number, t: BackupTargetInput) => request('PATCH', `/api/backups/${id}`, t),
  deleteBackup: (id: number) => request('DELETE', `/api/backups/${id}`),
  testBackup: (t: BackupTargetInput, id?: number) => request<{ ok: boolean; message: string }>('POST', '/api/backups/test', { ...t, id }),
  runBackup: (id: number) => request('POST', `/api/backups/${id}/run`),
  cancelBackup: () => request('POST', '/api/backups/cancel'),
  backupRuns: (id: number) => request<{ runs: BackupRun[] }>('GET', `/api/backups/${id}/runs`),
  backupVersions: (id: number) => request<{ versions: { id: string; label: string }[] }>('GET', `/api/backups/${id}/versions`),
  backupBrowse: (id: number, version: string, path: string) =>
    request<{ entries: BackupItem[] }>(
      'GET',
      `/api/backups/${id}/browse?version=${encodeURIComponent(version)}&path=${encodeURIComponent(path)}`,
    ),
  backupRestore: (id: number, body: { version: string; paths: string[]; mode: 'original' | 'folder' }) =>
    request('POST', `/api/backups/${id}/restore`, body),
  setupRestore: (t: Partial<BackupTargetInput>) => request('POST', '/api/setup/restore', t),
  setupRestoreStatus: () =>
    request<{ running: BackupProgress | null; result: RestoreResult | null; done?: boolean }>('GET', '/api/setup/restore'),

  // Live-Sync
  sync: () => request<{ rules: SyncRule[]; targets: SyncTarget[]; users: string[] }>('GET', '/api/sync'),
  createSync: (body: SyncRuleInput) => request<{ id: number }>('POST', '/api/sync', body),
  updateSync: (id: number, patch: { enabled?: boolean; keep_days?: number }) => request('PATCH', `/api/sync/${id}`, patch),
  deleteSync: (id: number) => request('DELETE', `/api/sync/${id}`),
  runSync: (id: number) => request('POST', `/api/sync/${id}/run`),

  // Eingang
  inbox: () => request<InboxStatus>('GET', '/api/inbox'),
  inboxUndo: (id: number) => request<{ ok: boolean }>('POST', `/api/inbox/${id}/undo`),

  // Dateien
  list: (path: string) => request<{ path: string; entries: Entry[] }>('GET', `/api/list?${q(path)}`),
  mkdir: (path: string) => request('POST', '/api/folder', { path }),
  rename: (from: string, to: string) => request('PATCH', '/api/rename', { from, to }),
  remove: (path: string) => request('DELETE', `/api/file?${q(path)}`),

  // Bibliothek
  timeline: (cursor?: string | null) =>
    request<{ entries: Entry[]; next: string | null; indexing: boolean }>(
      'GET',
      `/api/timeline${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ''}`,
    ),
  favorites: () => request<{ entries: Entry[] }>('GET', '/api/favorites'),
  setFavorite: (paths: string[], favorite: boolean) => request('POST', '/api/favorites', { paths, favorite }),
  albums: () => request<{ albums: Album[] }>('GET', '/api/albums'),
  album: (id: number) => request<{ id: number; name: string; entries: Entry[] }>('GET', `/api/albums/${id}`),
  createAlbum: (name: string) => request<{ id: number }>('POST', '/api/albums', { name }),
  renameAlbum: (id: number, name: string) => request('PATCH', `/api/albums/${id}`, { name }),
  deleteAlbum: (id: number) => request('DELETE', `/api/albums/${id}`),
  addToAlbum: (id: number, paths: string[]) => request<{ added: number }>('POST', `/api/albums/${id}/add`, { paths }),
  removeFromAlbum: (id: number, paths: string[]) => request('POST', `/api/albums/${id}/remove`, { paths }),

  // URLs
  fileUrl: (path: string) => `/api/file?${q(path)}`,
  /** PDF zum Anzeigen (Office-Dateien vom Server umgewandelt) */
  docUrl: (e: Entry) => `/api/doc?${q(e.path)}&${v(e)}`,
  archive: (path: string) =>
    request<{ entries: ArchiveItem[]; files: number; total_size: number; truncated: boolean }>('GET', `/api/archive?${q(path)}`),
  archiveFileUrl: (path: string, entry: string) => `/api/archive/file?${q(path)}&entry=${encodeURIComponent(entry)}`,
  downloadUrl: (path: string) => `/api/file?${q(path)}&download=true`,
  thumbUrl: (e: Entry) => `/api/thumb?${q(e.path)}&${v(e)}`,
  /** Bild in voller Größe; HEIC/TIFF als vom Server umgewandeltes JPEG */
  viewUrl: (e: Entry) => (e.preview ? `/api/preview?${q(e.path)}&${v(e)}` : `/api/file?${q(e.path)}`),
  /** Live-Photo-Video, vom Server nach H.264 umgewandelt */
  liveUrl: (e: Entry) => `/api/live?${q(e.path)}&${v(e)}`,

  /** Upload per XHR, weil fetch keinen Upload-Fortschritt kennt. */
  upload(dir: string, file: File, onProgress: (fraction: number) => void): Promise<void> {
    return new Promise((resolve, reject) => {
      const xhr = new XMLHttpRequest()
      xhr.open('POST', `/api/upload?${q(dir)}`)
      xhr.upload.onprogress = (e) => e.lengthComputable && onProgress(e.loaded / e.total)
      xhr.onload = () => {
        if (xhr.status >= 200 && xhr.status < 300) return resolve()
        if (xhr.status === 401) onUnauthorized()
        let msg = xhr.statusText
        try {
          msg = JSON.parse(xhr.responseText).error ?? msg
        } catch {}
        reject(new ApiError(xhr.status, msg))
      }
      xhr.onerror = () => reject(new ApiError(0, 'Netzwerkfehler'))
      const form = new FormData()
      form.append('file', file, file.name)
      xhr.send(form)
    })
  },
}

export const isMedia = (e: Entry) => e.kind === 'image' || e.kind === 'video'

export function joinPath(dir: string, name: string) {
  return dir ? `${dir}/${name}` : name
}

export function parentPath(path: string) {
  const i = path.lastIndexOf('/')
  return i < 0 ? '' : path.slice(0, i)
}

/** Nach dem Löschen: das nächste verbliebene Element (sonst das letzte) für die Lightbox */
export function nextRemaining(items: Entry[], current: string, removed: Set<string>): string | null {
  const i = items.findIndex((e) => e.path === current)
  const left = (e: Entry) => !removed.has(e.path)
  return (items.slice(i + 1).find(left) ?? items.filter(left).at(-1))?.path ?? null
}

export function formatSize(bytes: number) {
  if (bytes < 1024) return `${bytes} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let val = bytes / 1024
  let i = 0
  while (val >= 1024 && i < units.length - 1) {
    val /= 1024
    i++
  }
  return `${val.toFixed(val < 10 ? 1 : 0)} ${units[i]}`
}

/** Aufnahmezeit ist Ortszeit ohne Zone, daher immer als UTC formatieren. */
export function formatTaken(taken: number, opts: Intl.DateTimeFormatOptions) {
  return new Date(taken * 1000).toLocaleString('de-DE', { ...opts, timeZone: 'UTC' })
}

/** Woher Raster und Vollbildansicht ihre Bilder laden (eigene Dateien oder Teilen-Link). */
export interface MediaSource {
  thumb: (e: Entry) => string
  /** Bild in voller Größe (HEIC/TIFF als JPEG) */
  view: (e: Entry) => string
  /** Original zum Abspielen */
  file: (e: Entry) => string
  /** Download-Link oder null, wenn nicht erlaubt */
  download: (e: Entry) => string | null
  liveDownload: (e: Entry) => string | null
  live: (e: Entry) => string
  videoStatus: (e: Entry) => Promise<VideoStatus>
  web: (e: Entry) => string
}

export const userSource: MediaSource = {
  thumb: api.thumbUrl,
  view: api.viewUrl,
  file: (e) => api.fileUrl(e.path),
  download: (e) => api.downloadUrl(e.path),
  liveDownload: (e) => (e.live ? api.downloadUrl(e.live) : null),
  live: api.liveUrl,
  videoStatus: (e) => request<VideoStatus>('GET', `/api/video?${q(e.path)}`),
  web: (e) => `/api/video/web?${q(e.path)}&${v(e)}`,
}

export function shareSource(token: string, allowDownload: boolean): MediaSource {
  const base = (e: Entry) => `/api/s/${token}/${e.id}`
  return {
    thumb: (e) => `${base(e)}/thumb?${v(e)}`,
    view: (e) => `${base(e)}/view?${v(e)}`,
    file: (e) => `${base(e)}/file`,
    download: (e) => (allowDownload ? `${base(e)}/file?download=true` : null),
    liveDownload: () => null,
    live: (e) => `${base(e)}/live?${v(e)}`,
    videoStatus: (e) => request<VideoStatus>('GET', `${base(e)}/video`),
    web: (e) => `${base(e)}/web?${v(e)}`,
  }
}
