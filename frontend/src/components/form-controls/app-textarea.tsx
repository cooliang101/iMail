import type { ChangeEvent, CSSProperties, TextareaHTMLAttributes } from 'preact/compat';

type AppTextareaProps = Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, 'onChange'> & {
  resize?: CSSProperties['resize'];
  onChange?: (event: ChangeEvent<HTMLTextAreaElement>, data: { value: string }) => void;
};

export function AppTextarea({ className, resize, style, onChange, ...props }: AppTextareaProps) {
  return <span className={className ? `app-textarea ${className}` : 'app-textarea'}>
    <textarea {...props} autoComplete="off" data-form-type="other" data-lpignore="true" data-1p-ignore="true" style={{ ...(style as CSSProperties | undefined), resize }} onChange={(event) => onChange?.(event, { value: event.currentTarget.value })} />
  </span>;
}
