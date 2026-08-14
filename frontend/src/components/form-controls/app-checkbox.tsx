import { Checkbox, type CheckboxProps } from '@fluentui/react-components';

export function AppCheckbox({ className, ...props }: CheckboxProps) {
  const hasLabel = props.label !== undefined && props.label !== null && props.label !== '';
  const base = `app-checkbox${hasLabel ? ' has-label' : ''}`;
  return <Checkbox className={className ? `${base} ${className}` : base} {...props} />;
}
