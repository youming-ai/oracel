import type { DashboardStats, EquityPoint, HourlyStat, TradeEntry, TradeRecord, TradeSettlement } from '@/lib/dashboard-types'

function getHourLabel(time: string): string {
  const directMatch = time.match(/\b(\d{1,2}):\d{2}/)
  if (directMatch) {
    const hour = directMatch[1]
    return hour ? hour.padStart(2, '0') : '00'
  }

  return '00'
}

function findPendingEntries(trades: TradeRecord[]): TradeEntry[] {
  const pendingEntries: TradeEntry[] = []

  for (let index = trades.length - 1; index >= 0; index -= 1) {
    const trade = trades[index]

    if (trade.type === 'settlement') {
      break
    }

    if (trade.type === 'entry') {
      pendingEntries.push(trade)
    }
  }

  return pendingEntries.reverse()
}

export function computeStats(trades: TradeRecord[]): DashboardStats {
  const settlements = trades.filter((trade): trade is TradeSettlement => trade.type === 'settlement')
  const wins = settlements.filter((trade) => trade.result === 'WIN')
  const losses = settlements.filter((trade) => trade.result === 'LOSS')
  const pending = findPendingEntries(trades)

  const totalPnl = settlements.reduce((sum, trade) => sum + trade.pnl, 0)
  const winPnl = wins.reduce((sum, trade) => sum + trade.pnl, 0)
  const lossPnl = Math.abs(losses.reduce((sum, trade) => sum + trade.pnl, 0))
  const avgWin = wins.length > 0 ? winPnl / wins.length : 0
  const avgLoss = losses.length > 0 ? lossPnl / losses.length : 0

  let cumulativePnl = 0
  const equity: EquityPoint[] = settlements.map((trade, index) => {
    cumulativePnl += trade.pnl

    return {
      index,
      cumulativePnl,
      pnl: trade.pnl,
      time: trade.time,
    }
  })

  let peak = 0
  let maxDrawdown = 0

  for (const point of equity) {
    if (point.cumulativePnl > peak) {
      peak = point.cumulativePnl
    }

    const drawdown = peak - point.cumulativePnl
    if (drawdown > maxDrawdown) {
      maxDrawdown = drawdown
    }
  }

  const upTrades = settlements.filter((trade) => trade.direction === 'UP')
  const downTrades = settlements.filter((trade) => trade.direction === 'DOWN')

  const hourlyCounts: Record<string, { wins: number; losses: number }> = {}

  for (const settlement of settlements) {
    const hour = getHourLabel(settlement.time)

    if (!hourlyCounts[hour]) {
      hourlyCounts[hour] = { wins: 0, losses: 0 }
    }

    if (settlement.result === 'WIN') {
      hourlyCounts[hour].wins += 1
    } else {
      hourlyCounts[hour].losses += 1
    }
  }

  const hourlySeries: HourlyStat[] = Object.keys(hourlyCounts)
    .sort((a, b) => Number.parseInt(a, 10) - Number.parseInt(b, 10))
    .map((hour) => {
      const winsCount = hourlyCounts[hour].wins
      const lossesCount = hourlyCounts[hour].losses
      const total = winsCount + lossesCount

      return {
        hour: `${hour}:00`,
        wins: winsCount,
        losses: lossesCount,
        total,
        winRate: total > 0 ? (winsCount / total) * 100 : 0,
      }
    })

  let currentStreak = 0
  let maxWinStreak = 0
  let maxLossStreak = 0

  for (const settlement of settlements) {
    if (settlement.result === 'WIN') {
      currentStreak = currentStreak > 0 ? currentStreak + 1 : 1
      maxWinStreak = Math.max(maxWinStreak, currentStreak)
      continue
    }

    currentStreak = currentStreak < 0 ? currentStreak - 1 : -1
    maxLossStreak = Math.max(maxLossStreak, Math.abs(currentStreak))
  }

  const lastSettlement = settlements.length > 0 ? settlements[settlements.length - 1] : null

  const winRate = settlements.length > 0 ? (wins.length / settlements.length) * 100 : 0
  const profitFactor = lossPnl > 0 ? winPnl / lossPnl : Number.POSITIVE_INFINITY

  return {
    settlements,
    wins,
    losses,
    pending,
    totalPnl,
    winPnl,
    lossPnl,
    avgWin,
    avgLoss,
    equity,
    maxDrawdown,
    directionStats: [
      {
        direction: 'UP',
        wins: upTrades.filter((t) => t.result === 'WIN').length,
        losses: upTrades.filter((t) => t.result === 'LOSS').length,
      },
      {
        direction: 'DOWN',
        wins: downTrades.filter((t) => t.result === 'WIN').length,
        losses: downTrades.filter((t) => t.result === 'LOSS').length,
      },
    ],
    hourlySeries,
    streak: currentStreak,
    maxWinStreak,
    maxLossStreak,
    lastBTC: lastSettlement?.exitBTC ?? null,
    winRate,
    profitFactor,
  }
}
