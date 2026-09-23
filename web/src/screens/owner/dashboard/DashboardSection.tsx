import type { ReactNode } from "react";

/**
 * One headed section of the Dashboard: a title, an optional hint under it
 * that says how to read the figures, and the section's card or table. The
 * three sections share this shape, so the heading and hint are written once.
 */
export function DashboardSection({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <section className="mt-6">
      <h4 className="m-0 text-[0.938rem] font-semibold text-text">{title}</h4>
      {hint ? <p className="mt-1 text-[0.813rem] text-muted">{hint}</p> : null}
      <div className="mt-3">{children}</div>
    </section>
  );
}
