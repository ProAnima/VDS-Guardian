export function BrandMark({ label }: { label: string }) {
  return (
    <svg className="brand__mark" viewBox="0 0 48 48" role="img" aria-label={label}>
      <path d="M24 3 42 10v14c0 12-7 19-18 24C13 43 6 36 6 24V10Z" />
      <path className="brand__v" d="m14 16 10 22 10-22h-7l-3 9-3-9Z" />
      <circle cx="24" cy="13" r="2.5" />
    </svg>
  );
}
