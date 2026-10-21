/** The mark: a chain of stars — three belt stars, two bright anchors, one shoulder. */
export function RigelMark({ size = 18 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden>
      <path
        d="M7.2 5.6 10.6 12.4 12.6 11.9 14.6 11.4 17.4 18.6"
        stroke="currentColor"
        strokeWidth="1.7"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <g fill="currentColor">
        <circle cx="10.6" cy="12.4" r="1.7" />
        <circle cx="12.6" cy="11.9" r="1.7" />
        <circle cx="14.6" cy="11.4" r="1.7" />
        <circle cx="15.4" cy="6.2" r="1.1" />
        <circle cx="7.2" cy="5.6" r="2.4" />
        <circle cx="17.4" cy="18.6" r="2.2" />
      </g>
    </svg>
  );
}
