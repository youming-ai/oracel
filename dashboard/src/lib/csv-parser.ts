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
      // New format (7 cols): timestamp,result,direction,,pnl,entry_btc,exit_btc
      // Old format (6 cols): timestamp,result,direction,pnl,entry_btc,exit_btc
      const hasOrderId = cols.length >= 7 && cols[3]?.trim() === ''
      const pnlIdx = hasOrderId ? 4 : 3
      const settlement: TradeSettlement = {
        type: 'settlement',
        time,
        result: first,
        direction: (cols[2]?.trim().toUpperCase() ?? 'UP') as TradeDirection,
        pnl: parseNumber(cols[pnlIdx]) ?? 0,
        entryBTC: parseInteger(cols[pnlIdx + 1]),
        exitBTC: parseInteger(cols[pnlIdx + 2]),
        price: pendingEntry?.price ?? null,
        edge: pendingEntry?.edge ?? null,
        payoff: pendingEntry?.payoff ?? null,
      }

      trades.push(settlement)
      pendingEntry = null
      continue
    }

    if (first === 'ENTRY' || first === 'UP' || first === 'DOWN') {
      // New format (12 cols): timestamp,ENTRY,direction,order_id,price,cost,edge,balance,ttl,yes,no,payoff
      // Old format (11 cols): timestamp,direction,order_id,price,cost,edge,balance,ttl,yes,no,payoff
      const isNew = first === 'ENTRY'
      const off = isNew ? 0 : -1
      const direction = isNew
        ? (cols[2]?.trim().toUpperCase() ?? 'UP') as TradeDirection
        : first

      const rawTtl = cols[8 + off]?.trim() ?? ''
      const ttl = rawTtl.endsWith('s') ? rawTtl.slice(0, -1) : rawTtl || null

      const payoffIdx = isNew ? 11 : 10
      const rawPayoff = cols.length > payoffIdx ? (cols[payoffIdx]?.trim() ?? '') : ''
      const payoff = rawPayoff.endsWith('x') ? rawPayoff.slice(0, -1) : rawPayoff || null

      const entry: TradeEntry = {
        type: 'entry',
        time,
        direction,
        conditionId: cols[3 + off]?.trim() ?? '',
        price: parseNumber(cols[4 + off]),
        cost: parseNumber(cols[5 + off]),
        edge: parseNumber(cols[6 + off]),
        balance: parseNumber(cols[7 + off]),
        ttl,
        payoff,
      }

      trades.push(entry)
      pendingEntry = entry
    }
  }

  return trades
}
