import { defineConfig, type Plugin } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'node:path'
import fs from 'node:fs'
import tailwindcss from '@tailwindcss/vite'

function detectBotMode(): string {
  if (process.env.BOT_MODE) return process.env.BOT_MODE
  try {
    const configPath = path.resolve(__dirname, '../config.json')
    const config = JSON.parse(fs.readFileSync(configPath, 'utf-8'))
    return config?.trading?.mode ?? 'paper'
  } catch {
    return 'paper'
  }
}

const BOT_MODE = detectBotMode()
const LOGS_DIR = path.resolve(__dirname, `../logs/${BOT_MODE}`)

function botData(): Plugin {
  return {
    name: 'bot-data-proxy',
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        if (req.url === '/trades.csv' || req.url === '/balance' || req.url === '/time_windows.json') {
          const filePath = path.join(LOGS_DIR, req.url)
          if (fs.existsSync(filePath)) {
            res.setHeader('Content-Type', req.url.endsWith('.json') ? 'application/json' : 'text/plain')
            res.setHeader('Cache-Control', 'no-store')
            fs.createReadStream(filePath).pipe(res)
            return
          }
          res.statusCode = 404
          res.end('not found')
          return
        }
        next()
      })
    },
  }
}

export default defineConfig({
  base: './',
  plugins: [react(), tailwindcss(), botData()],
  define: {
    __BOT_MODE__: JSON.stringify(BOT_MODE),
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    allowedHosts: ['orakel.um1ng.me'],
  },
})
