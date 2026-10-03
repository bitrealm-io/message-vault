import { useState } from "react";

/**
 * Run `reset` while rendering, once each time `scope` changes, so state that
 * belongs to what a screen shows starts again the moment the screen shows
 * something else. Setting state during render is React's way to do this
 * without an effect: the component renders once more, with the reset state,
 * before anything is painted.
 *
 * `scope` lists what the state belongs to (a query, a filter, a group) and is
 * compared by value.
 */
export function useResetOnChange(scope: readonly unknown[], reset: () => void): void {
  const key = JSON.stringify(scope);
  const [seen, setSeen] = useState(key);
  if (seen !== key) {
    setSeen(key);
    reset();
  }
}
