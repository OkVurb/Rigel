/** Rigel's mark: a single bright star, the anchor the name points to. */
export function RigelMark({ size = 18 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden>
      <circle cx="12" cy="12" r="5.5" fill="currentColor" fillOpacity="0.14" />
      <path
        d="M12 4 Q12.9 11.1 20 12 Q12.9 12.9 12 20 Q11.1 12.9 4 12 Q11.1 11.1 12 4 Z"
        fill="currentColor"
      />
    </svg>
  );
}
