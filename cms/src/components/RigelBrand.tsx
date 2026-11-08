// The mark is the same geometry as web/rigel.svg, inlined so the admin needs no image request
// and so the login wordmark can set the name in the interface's own type.
type MarkProps = {
  label?: string
  size?: number
}

export function RigelMark({ label, size = 28 }: MarkProps) {
  return (
    <svg
      aria-hidden={label ? undefined : true}
      aria-label={label}
      className="rigel-mark"
      focusable="false"
      height={size}
      role={label ? 'img' : undefined}
      viewBox="0 0 24 24"
      width={size}
    >
      <rect fill="#0b0c0e" height="24" rx="5" width="24" />
      <circle cx="12" cy="12" fill="#e9a23b" fillOpacity="0.14" r="5.5" />
      <path
        d="M12 4 Q12.9 11.1 20 12 Q12.9 12.9 12 20 Q11.1 12.9 4 12 Q11.1 11.1 12 4 Z"
        fill="#e9a23b"
      />
    </svg>
  )
}

/** The login screen's mark: icon plus the product name, not a replacement wordmark. */
export function RigelLogo() {
  return (
    <span className="rigel-brand">
      <RigelMark size={44} />
      <span className="rigel-brand__word">
        <strong>Rigel</strong>
        <small>Content</small>
      </span>
    </span>
  )
}

/** The compact mark in the navigation rail. */
export function RigelIcon() {
  return <RigelMark label="Rigel" size={24} />
}
