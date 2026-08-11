import { forwardRef, useMemo, useState, type CSSProperties, type InputHTMLAttributes } from 'react';
import {
  Checkbox,
  Dropdown,
  Input,
  Option,
  Textarea,
  type CheckboxProps,
  type DropdownProps,
  type InputProps,
  type TextareaProps,
} from '@fluentui/react-components';

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

export function AppTextarea({ className, ...props }: TextareaProps) {
  return <Textarea appearance="outline" className={classes('app-textarea', className)} {...props} />;
}

export function AppCheckbox({ className, ...props }: CheckboxProps) {
  const hasLabel = props.label !== undefined && props.label !== null && props.label !== '';
  return <Checkbox className={classes(`app-checkbox${hasLabel ? ' has-label' : ''}`, className)} {...props} />;
}

export type AppSelectOption = { value: string; label: string; disabled?: boolean };

type AppSelectProps = Omit<DropdownProps, 'children' | 'value' | 'defaultValue' | 'selectedOptions' | 'defaultSelectedOptions' | 'onOptionSelect'> & {
  name?: string;
  value?: string;
  defaultValue?: string;
  options: AppSelectOption[];
  onValueChange?: (value: string) => void;
};

export function AppSelect({ name, value, defaultValue, options, onValueChange, className, ...props }: AppSelectProps) {
  const initialValue = defaultValue ?? options.find((option) => !option.disabled)?.value ?? '';
  const [internalValue, setInternalValue] = useState(initialValue);
  const selectedValue = value ?? internalValue;
  const selectedLabel = useMemo(() => options.find((option) => option.value === selectedValue)?.label ?? '', [options, selectedValue]);

  return <span className="app-select-wrap">
    <Dropdown
      {...props}
      className={classes('app-select', className)}
      selectedOptions={selectedValue ? [selectedValue] : []}
      value={selectedLabel}
      onOptionSelect={(_, data) => {
        const next = String(data.optionValue ?? '');
        if (value === undefined) setInternalValue(next);
        onValueChange?.(next);
      }}
    >
      {options.map((option) => <Option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</Option>)}
    </Dropdown>
    {name && <input type="hidden" name={name} value={selectedValue} />}
  </span>;
}
