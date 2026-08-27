import type { ChangeEvent, InputHTMLAttributes, ReactNode } from 'preact/compat';

type AppSwitchProps = Omit<InputHTMLAttributes<HTMLInputElement>, 'type' | 'onChange'> & {
  label?: ReactNode;
  onChange?: (event: ChangeEvent<HTMLInputElement>, data: { checked: boolean }) => void;
};

export function AppSwitch({ className, label, onChange, ...props }: AppSwitchProps) {
  const hasLabel = label !== undefined && label !== null && label !== '';
  const base = `app-switch${hasLabel ? ' has-label' : ''}`;
  const control = <>
    <input {...props} type="checkbox" role="switch" onChange={(event) => onChange?.(event, { checked: event.currentTarget.checked })} />
    <span className="app-switch-track" aria-hidden="true" />
  </>;

  if (hasLabel) return <label className={className ? `${base} ${className}` : base}>{control}<span className="app-switch-label">{label}</span></label>;
  return <span className={className ? `${base} ${className}` : base}>{control}</span>;
}
