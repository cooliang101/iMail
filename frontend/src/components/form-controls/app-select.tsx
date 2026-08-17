import { useState, type SelectHTMLAttributes } from 'preact/compat';

export type AppSelectOption = { value: string; label: string; disabled?: boolean };

type AppSelectProps = Omit<SelectHTMLAttributes<HTMLSelectElement>, 'children' | 'value' | 'defaultValue' | 'onChange'> & {
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

  return <span className="app-select-wrap">
    <select
      {...props}
      name={name}
      className={className ? `app-select ${className}` : 'app-select'}
      value={selectedValue}
      onChange={(event) => {
        const next = event.currentTarget.value;
        if (value === undefined) setInternalValue(next);
        onValueChange?.(next);
      }}
    >
      {options.map((option) => <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>)}
    </select>
  </span>;
}
