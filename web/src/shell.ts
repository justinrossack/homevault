import { createContext, useContext } from 'react'
import type { Entry, User } from './api'
import type { ShareTarget } from './components/ShareDialog'

/** Was alle Ansichten brauchen: Benutzer, Hinweise, Favoriten, Alben. */
export interface ShellApi {
  user: User
  serverName: string
  toast: (msg: string) => void
  /** Setzt/entfernt Favoriten und meldet Fehler selbst. Gibt true bei Erfolg zurück. */
  setFavorite: (entries: Entry[], favorite: boolean) => Promise<boolean>
  /** Öffnet die Album-Auswahl für diese Einträge. */
  addToAlbum: (entries: Entry[]) => void
  /** Fragt nach und löscht endgültig. `onDeleted` bekommt die gelöschten Pfade. */
  deleteEntries: (entries: Entry[], onDeleted: (paths: Set<string>) => void) => void
  /** Öffnet den Teilen-Dialog */
  share: (target: ShareTarget) => void
  refreshMe: () => void
}

export const ShellContext = createContext<ShellApi | null>(null)

export function useShell(): ShellApi {
  const ctx = useContext(ShellContext)
  if (!ctx) throw new Error('useShell außerhalb von <Shell>')
  return ctx
}
