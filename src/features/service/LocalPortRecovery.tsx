import { Button } from '@fluentui/react-components';
import { ArrowsClockwise, Plugs } from '@phosphor-icons/react';
import { AppInput } from '../../components/form-controls';
import { DEFAULT_LOCAL_SERVICE_PORT, MAX_LOCAL_SERVICE_PORT, MIN_LOCAL_SERVICE_PORT } from '../../service-config';

export function localPortConflictMessage(reason: unknown) {
  const message = reason instanceof Error ? reason.message : String(reason);
  return /端口.*(?:占用|冲突)|(?:占用|冲突).*端口/.test(message);
}

export function suggestedLocalServicePort(port: number) {
  return port >= MAX_LOCAL_SERVICE_PORT ? DEFAULT_LOCAL_SERVICE_PORT : port + 1;
}

export function LocalPortRecovery({ port, busy, onPortChange, onRetry }: {
  port: string;
  busy: boolean;
  onPortChange: (port: string) => void;
  onRetry: () => void;
}) {
  return <section className="local-port-recovery" aria-labelledby="local-port-recovery-title">
    <Plugs size={19} weight="duotone" />
    <div>
      <strong id="local-port-recovery-title">换一个本地端口</strong>
      <p>原端口被其他程序占用。服务仍只监听 127.0.0.1，邮件数据和用户级守护配置不会改变位置。</p>
      <label>
        <span>本地服务端口</span>
        <AppInput
          type="number"
          inputMode="numeric"
          min={MIN_LOCAL_SERVICE_PORT}
          max={MAX_LOCAL_SERVICE_PORT}
          step={1}
          value={port}
          onChange={(_, data) => onPortChange(data.value)}
          disabled={busy}
          aria-describedby="local-port-range"
        />
      </label>
      <small id="local-port-range">可选择 {MIN_LOCAL_SERVICE_PORT}–{MAX_LOCAL_SERVICE_PORT} 之间未占用的端口。</small>
      <Button type="button" appearance="primary" icon={<ArrowsClockwise size={16} />} onClick={onRetry} disabled={busy}>
        {busy ? '正在切换端口…' : '使用此端口并启动'}
      </Button>
    </div>
  </section>;
}
