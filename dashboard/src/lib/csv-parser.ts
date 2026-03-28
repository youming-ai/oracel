import type { TradeDirection, TradeEntry, TradeRecord, TradeSettlement } from '@/lib/dashboard-types'

function parseNumber(value: string | undefined): number | null {
  if (!value) {
    return null
  }

  const parsed = Number.parseFloat(value.trim())
  return Number.isFinite(parsed) ? parsed : null
}

function parseInteger(value: string | undefined): number | null {
  if (!value) {
    return null
  }

  const parsed = Number.parseInt(value.trim(), 10)
  return Number.isFinite(parsed) ? parsed : null
}

export function parseTrades(csvText: string): TradeRecord[] {
  const trimmed = csvText.trim()

  if (!trimmed) {
    return []
  }

  const lines = trimmed.split('\n').map((line) => line.trim()).filter(Boolean)
  const trades: TradeRecord[] = []
  let pendingEntry: TradeEntry | null = null

  for (const line of lines) {
    const cols = line.split(',')
    if (cols.length < 3) {
      console.warn(`[csv-parser] Skipping malformed line (${cols.length} columns): ${line.slice(0, 80)}`)
      continue
    }

    const time = cols[0]?.trim() ?? ''
    const first = cols[1]?.trim().toUpperCase()

    if (first === 'WIN' || first === 'LOSS') {
      const settlement: TradeSettlement = {
        type: 'settlement',
        time,
        result: first,
        direction: (cols[2]?.trim().toUpperCase() ?? 'UP') as TradeDirection,
        pnl: parseNumber(cols[3]) ?? 0,
        entryBTC: parseInteger(cols[4]),
        exitBTC: parseInteger(cols[5]),
        price: pendingEntry?.price ?? null,
        edge: pendingEntry?.edge ?? null,
        payoff: pendingEntry?.payoff ?? null,
      }

      trades.push(settlement)
      pendingEntry = null
      continue
    }

    if (first === 'UP' || first === 'DOWN') {
      const rawTtl = cols[7]?.trim() ?? ''
      const ttl = rawTtl.endsWith('s') ? rawTtl.slice(0, -1) : rawTtl || null

      const rawPayoff = cols.length > 10 ? (cols[10]?.trim() ?? '') : ''
      const payoff = rawPayoff.endsWith('x') ? rawPayoff.slice(0, -1) : rawPayoff || null

      const entry: TradeEntry = {
        type: 'entry',
        time,
        direction: first,
        conditionId: cols[2]?.trim() ?? '',
        price: parseNumber(cols[3]),
        cost: parseNumber(cols[4]),
        edge: parseNumber(cols[5]),
        balance: parseNumber(cols[6]),
        ttl,
        payoff,
      }

      trades.push(entry)
      pendingEntry = entry
    }
  }

  return trades
}
