import PlainButton from "../../components/PlainButton";

export default function YearChipBar({
  years,
  activeYear,
  onSelectAll,
  onSelectYear,
}: {
  years: number[];
  activeYear: number | null;
  onSelectAll: () => void;
  onSelectYear: (year: number) => void;
}) {
  if (years.length === 0) return null;

  const chipClass = (active: boolean) =>
    `cursor-pointer rounded border px-1.5 py-0.5 text-[0.688rem] ${
      active
        ? "border-accent bg-accent font-semibold text-[var(--sent-text,#fff)]"
        : "border-border bg-panel font-normal text-accent"
    }`;

  return (
    <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
      <PlainButton
        onPress={onSelectAll}
        title="Show all years (paged)"
        className={chipClass(activeYear === null)}
      >
        All
      </PlainButton>
      {years.map((year) => (
        <PlainButton
          key={year}
          onPress={() => onSelectYear(year)}
          title={activeYear === year ? `Clear ${year} filter` : `Load all messages from ${year}`}
          className={chipClass(activeYear === year)}
        >
          {year}
        </PlainButton>
      ))}
    </div>
  );
}
