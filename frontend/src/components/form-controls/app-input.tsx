import { forwardRef, type CSSProperties, type InputHTMLAttributes } from 'react';
import { Input, type InputProps } from '@fluentui/react-components';

function classes(base: string, className?: string) {
  return className ? `${base} ${className}` : base;
}

export const AppInput = forwardRef<HTMLInputElement, InputProps>(function AppInput({ className, ...props }, ref) {
  return <Input ref={ref} appearance="outline" className={classes('app-input', className)} {...props} />;
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
