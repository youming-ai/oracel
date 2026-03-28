import { Component, type ErrorInfo, type ReactNode } from 'react'

interface Props {
  children: ReactNode
}

interface State {
  hasError: boolean
  error: Error | null
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props)
    this.state = { hasError: false, error: null }
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('[ErrorBoundary]', error, info.componentStack)
  }

  render() {
    if (this.state.hasError) {
      return (
        <div style={{ padding: '2rem', color: '#ff4757', fontFamily: 'monospace' }}>
          <h2>Dashboard Error</h2>
          <pre style={{ whiteSpace: 'pre-wrap', opacity: 0.8 }}>
            {this.state.error?.message ?? 'Unknown error'}
          </pre>
          <button
            style={{ marginTop: '1rem', padding: '0.5rem 1rem', cursor: 'pointer' }}
            onClick={() => this.setState({ hasError: false, error: null })}
          >
            Retry
          </button>
        </div>
      )
    }

    return this.props.children
  }
}
