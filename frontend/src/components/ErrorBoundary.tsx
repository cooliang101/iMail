import { Component, type ErrorInfo, type ReactNode } from 'preact/compat';
import { ArrowCounterClockwise, WarningCircle, X } from './icons';
import { desktopLog, describeDesktopLogValue } from '../services';

type ErrorBoundaryProps = {
  children: ReactNode;
  scope: string;
  resetKey?: unknown;
  fallback: (reset: () => void) => ReactNode;
};

type ErrorBoundaryState = { failed: boolean };

export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { failed: false };

  static getDerivedStateFromError(): ErrorBoundaryState {
    return { failed: true };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    void desktopLog('error', 'frontend.error_boundary', [
      `scope=${this.props.scope}`,
      describeDesktopLogValue(error),
      info.componentStack ?? '',
    ].filter(Boolean).join('\n'));
  }

  componentDidUpdate(previous: ErrorBoundaryProps) {
    if (this.state.failed && previous.resetKey !== this.props.resetKey) this.setState({ failed: false });
  }

  private readonly reset = () => this.setState({ failed: false });

  render() {
    return this.state.failed ? this.props.fallback(this.reset) : this.props.children;
  }
}

export function AppErrorBoundary({ children }: { children: ReactNode }) {
  return <ErrorBoundary scope="app" fallback={(reset) => <main className="app-error-screen" role="alert">
    <section>
      <WarningCircle size={42} weight="duotone" />
      <span>界面保护已启动</span>
      <h1>应用界面遇到错误</h1>
      <p>异常区域已被安全卸载，避免继续破坏页面布局。你可以重新挂载界面；若问题重复出现，请从服务连接页检查后台状态。</p>
      <button type="button" onClick={reset}><ArrowCounterClockwise size={17} />重新挂载界面</button>
    </section>
  </main>}>{children}</ErrorBoundary>;
}

export function FeatureErrorBoundary({ label, onClose, resetKey, children }: { label: string; onClose: () => void; resetKey?: unknown; children: ReactNode }) {
  return <ErrorBoundary scope={`feature:${label}`} resetKey={resetKey} fallback={(reset) => <div className="feature-error-overlay" role="dialog" aria-modal="true" aria-label={`${label}加载失败`}>
    <section>
      <button type="button" className="feature-error-close" aria-label={`关闭${label}`} onClick={onClose}><X size={18} /></button>
      <WarningCircle size={34} weight="duotone" />
      <h2>{label}暂时无法显示</h2>
      <p>这个功能发生了渲染错误，主界面仍可继续使用。</p>
      <button type="button" onClick={reset}><ArrowCounterClockwise size={16} />重试此功能</button>
    </section>
  </div>}>{children}</ErrorBoundary>;
}

export function WorkspaceErrorBoundary({ label, resetKey, children }: { label: string; resetKey?: unknown; children: ReactNode }) {
  return <ErrorBoundary scope={`workspace:${label}`} resetKey={resetKey} fallback={(reset) => <section className="workspace-error" role="alert">
    <WarningCircle size={38} weight="duotone" />
    <h2>{label}暂时无法显示</h2>
    <p>该区域发生了渲染错误，侧栏和设置仍可继续使用。错误详情已写入应用日志。</p>
    <button type="button" onClick={reset}><ArrowCounterClockwise size={16} />重试此区域</button>
  </section>}>{children}</ErrorBoundary>;
}
