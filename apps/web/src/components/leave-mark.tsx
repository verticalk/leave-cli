interface LeaveMarkProps {
  size?: number;
  className?: string;
}

export function LeaveMark({ size = 24, className }: LeaveMarkProps) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="none"
      height={size}
      viewBox="0 0 32 32"
      width={size}
    >
      <circle cx="9" cy="6.5" fill="currentColor" r="1.75" />
      <path
        d="M9 9v8.5c0 3 1.5 4.5 4.5 4.5H23M18.5 17.5 23 22l-4.5 4.5"
        stroke="currentColor"
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth="2.25"
      />
    </svg>
  );
}
