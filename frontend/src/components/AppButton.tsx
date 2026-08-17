import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from 'preact/compat';

export type AppButtonAppearance = 'primary' | 'secondary' | 'subtle';

export type AppButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  appearance?: AppButtonAppearance;
  icon?: ReactNode;
};

export const AppButton = forwardRef<HTMLButtonElement, AppButtonProps>(function AppButton({ appearance = 'secondary', className, icon, type = 'button', children, ...props }, ref) {
  const classes = ['app-button', `is-${appearance}`, className].filter(Boolean).join(' ');
  return <button {...props} ref={ref} type={type} className={classes}>
    {icon && <span className="app-button-icon" aria-hidden="true">{icon}</span>}
    {children}
  </button>;
});
