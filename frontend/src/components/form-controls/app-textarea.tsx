import { Textarea, type TextareaProps } from '@fluentui/react-components';

export function AppTextarea({ className, ...props }: TextareaProps) {
  return <Textarea appearance="outline" className={className ? `app-textarea ${className}` : 'app-textarea'} {...props} />;
}
