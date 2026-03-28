export type TradeDirection = 'UP' | 'DOWN'
export type TradeResult = 'WIN' | 'LOSS'

export interface TradeEntry {
  type: 'entry'
  time: string
  direction: TradeDirection
  conditionId: string
  price: number | null
  cost: number | null
  edge: number | null
  balance: number | null
  ttl: string | null
  payoff: string | null
}

export interface TradeSettlement {
  type: 'settlement'
  time: string
  result: TradeResult
  direction: TradeDirection
  pnl: number
  entryBTC: number | null
  exitBTC: number | null
  price: number | null
  edge: number | null
  payoff: string | null
}

export type TradeRecord = TradeEntry | TradeSettlement

export interface DirectionStats {
  direction: TradeDirection
  wins: number
  losses: number
}

export interface HourlyStat {
  hour: string
  wins: number
  losses: number
  total: number
  winRate: number
}

export interface EquityPoint {
  index: number
  cumulativePnl: number
  pnl: number
  time: string
}

export interface DashboardStats {
  settlements: TradeSettlement[]
  wins: TradeSettlement[]
  losses: TradeSettlement[]
  pending: TradeEntry[]
  totalPnl: number
  winPnl: number
  lossPnl: number
  avgWin: number
  avgLoss: number
  equity: EquityPoint[]
  maxDrawdown: number
  directionStats: DirectionStats[]
  hourlySeries: HourlyStat[]
  streak: number
  maxWinStreak: number
  maxLossStreak: number
  lastBTC: number | null
  winRate: number
  profitFactor: number
}
