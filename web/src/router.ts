import { useEffect, useState } from 'react'

/**
 * Minimaler Hash-Router, damit Zurück-Button und Lesezeichen funktionieren:
 *   #/files/<pfad>  #/timeline  #/favorites  #/albums  #/albums/<id>  #/trash  #/settings
 */
export type Route =
  | { view: 'files'; path: string }
  | { view: 'timeline' }
  | { view: 'favorites' }
  | { view: 'albums' }
  | { view: 'album'; id: number }
  | { view: 'settings'; tab?: string }
  | { view: 'trash' }

/** Kaputte Prozent-Kodierung (z. B. „%E0%A4%A“) nicht abstürzen lassen */
function safeDecode(s: string) {
  try {
    return decodeURIComponent(s)
  } catch {
    return s
  }
}

export function parseHash(hash: string): Route {
  const parts = hash.replace(/^#\/?/, '').split('/')
  const [first, ...rest] = parts
  switch (first) {
    case 'timeline':
      return { view: 'timeline' }
    case 'favorites':
      return { view: 'favorites' }
    case 'settings':
      return { view: 'settings', tab: rest[0] || undefined }
    case 'trash':
      return { view: 'trash' }
    case 'albums': {
      const id = Number(rest[0])
      return rest[0] && Number.isFinite(id) ? { view: 'album', id } : { view: 'albums' }
    }
    case 'files':
      return { view: 'files', path: rest.map(safeDecode).join('/') }
    default:
      return { view: 'files', path: '' }
  }
}

export function hrefFor(route: Route): string {
  switch (route.view) {
    case 'files':
      return '#/files/' + route.path.split('/').filter(Boolean).map(encodeURIComponent).join('/')
    case 'album':
      return `#/albums/${route.id}`
    case 'settings':
      return route.tab ? `#/settings/${route.tab}` : '#/settings'
    default:
      return `#/${route.view}`
  }
}

export function navigate(route: Route) {
  location.hash = hrefFor(route)
}

export function useRoute(): Route {
  const [route, setRoute] = useState(() => parseHash(location.hash))
  useEffect(() => {
    const onHash = () => setRoute(parseHash(location.hash))
    window.addEventListener('hashchange', onHash)
    return () => window.removeEventListener('hashchange', onHash)
  }, [])
  return route
}
