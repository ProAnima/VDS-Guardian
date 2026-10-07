import { useRef } from "react";

/**
 * The latest value without making it an effect dependency. Used for the translator: switching
 * the language must re-render text, not re-run loads (which would collapse the explorer tree and
 * read the server again).
 */
export function useLatest<T>(value: T) {
  const ref = useRef(value);
  ref.current = value;
  return ref;
}
