import { forwardRef, type ChangeEvent, type CSSProperties, type InputHTMLAttributes, type ReactNode } from 'preact/compat';

function classes(base: string, className: unknown) {
  return typeof className === 'string' && className ? `${base} ${className}` : base;
}

type AppInputProps = Omit<InputHTMLAttributes<HTMLInputElement>, 'size' | 'onChange'> & {
  contentBefore?: ReactNode;
  contentAfter?: ReactNode;
  onChange?: (event: ChangeEvent<HTMLInputElement>, data: { value: string }) => void;
};

export const AppInput = forwardRef<HTMLInputElement, AppInputProps>(function AppInput({ className, contentBefore, contentAfter, onChange, ...props }, ref) {
  return <span className={classes('app-input', className)}>
    {contentBefore && <span className="app-input-decoration" aria-hidden="true">{contentBefore}</span>}
    <input ref={ref} {...props} onChange={(event) => onChange?.(event, { value: event.currentTarget.value })} />
    {contentAfter && <span className="app-input-decoration app-input-decoration-after">{contentAfter}</span>}
  </span>;
});

type AppColorInputProps = Omit<InputHTMLAttributes<HTMLInputElement>, 'type' | 'value' | 'onChange'> & {
  value: string;
  onValueChange?: (value: string) => void;
};

export const AppColorInput = forwardRef<HTMLInputElement, AppColorInputProps>(function AppColorInput({ className, value, onValueChange, ...props }, ref) {
  const pickerValue = /^#[0-9a-fA-F]{6}$/.test(value) ? value : '#000000';
  return <span className={classes('app-color-input', className)} style={{ '--app-color-value': pickerValue } as CSSProperties}>
    <span className="app-color-input-swatch" aria-hidden="true" />
    <input {...props} ref={ref} type="color" value={pickerValue} onChange={(event) => onValueChange?.(event.currentTarget.value)} />
  </span>;
});
