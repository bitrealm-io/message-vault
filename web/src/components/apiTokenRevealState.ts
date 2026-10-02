import { createContext, useContext } from "react";

export type ApiTokenReveal = { label: string; token: string };

/** Set by `ApiTokenRevealProvider`; opens its reveal dialog. */
export const ApiTokenRevealContext = createContext<((reveal: ApiTokenReveal) => void) | null>(null);

/** Opens the reveal dialog of the nearest `ApiTokenRevealProvider`. */
export function useRevealApiToken(): (reveal: ApiTokenReveal) => void {
  const reveal = useContext(ApiTokenRevealContext);
  if (reveal === null) throw new Error("useRevealApiToken needs an ApiTokenRevealProvider");
  return reveal;
}
