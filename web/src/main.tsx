import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { CssBaseline, ThemeProvider, createTheme } from '@mui/material'
import '@fontsource-variable/roboto-flex'
import App from './App'

const theme = createTheme({
  cssVariables: { colorSchemeSelector: 'class' },
  colorSchemes: {
    light: { palette: { primary: { main: '#3f5bd8' }, background: { default: '#f6f7fb' } } },
    dark: { palette: { primary: { main: '#9fb0ff' }, background: { default: '#111216', paper: '#1a1b21' } } },
  },
  shape: { borderRadius: 14 },
  typography: { fontFamily: '"Roboto Flex Variable", system-ui, sans-serif' },
  components: {
    MuiButton: { styleOverrides: { root: { textTransform: 'none', fontWeight: 600 } } },
  },
})

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ThemeProvider theme={theme} defaultMode="system">
      <CssBaseline />
      <App />
    </ThemeProvider>
  </StrictMode>,
)
