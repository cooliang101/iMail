import { useMemo, useState } from 'react';
import { Dropdown, Option, type DropdownProps } from '@fluentui/react-components';

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
      className={className ? `app-select ${className}` : 'app-select'}
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
