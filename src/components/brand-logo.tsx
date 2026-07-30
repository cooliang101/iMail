type BrandLogoProps = {
  className?: string;
  label?: string;
};

export function BrandLogo({ className = '', label }: BrandLogoProps) {
  return <svg
    className={`brand-logo ${className}`.trim()}
    viewBox="0 0 24 24"
    role={label ? 'img' : undefined}
    aria-label={label}
    aria-hidden={label ? undefined : true}
  >
    <path d="M6.25 4.75h11.5A3.25 3.25 0 0 1 21 8v8a3.25 3.25 0 0 1-3.25 3.25H6.25A3.25 3.25 0 0 1 3 16V8a3.25 3.25 0 0 1 3.25-3.25Z" fill="none" stroke="currentColor" strokeWidth="1.75" />
    <path d="m4.25 7.15 5.85 4.7m9.65-4.7-5.85 4.7" fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" strokeWidth="1.75" />
    <circle cx="12" cy="9.55" r="1" fill="currentColor" />
    <path d="M12 12.65v4.1" fill="none" stroke="currentColor" strokeLinecap="round" strokeWidth="2" />
  </svg>;
}
