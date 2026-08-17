import type { ChangeEvent, InputHTMLAttributes, ReactNode } from 'preact/compat';

type AppCheckboxProps = Omit<InputHTMLAttributes<HTMLInputElement>, 'type' | 'onChange'> & {
  label?: ReactNode;
  onChange?: (event: ChangeEvent<HTMLInputElement>, data: { checked: boolean }) => void;
};

export function AppCheckbox({ className, label, onChange, ...props }: AppCheckboxProps) {
  const hasLabel = label !== undefined && label !== null && label !== '';
  const base = `app-checkbox${hasLabel ? ' has-label' : ''}`;
  const control = <input {...props} type="checkbox" onChange={(event) => onChange?.(event, { checked: event.currentTarget.checked })} />;
  if (hasLabel) return <label className={className ? `${base} ${className}` : base}>{control}<span>{label}</span></label>;
  return <span className={className ? `${base} ${className}` : base}>{control}</span>;
}
