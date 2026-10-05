export const StatCard = (props: {
  label: string;
  value: string;
  hint?: string;
}) => (
  <div className="rounded-lg border bg-card p-4 text-card-foreground shadow-sm">
    <div className="text-sm text-muted-foreground">{props.label}</div>
    <div className="mt-1 text-2xl font-semibold tabular-nums">{props.value}</div>
    {props.hint
      ? (
          <div className="mt-1 text-xs text-muted-foreground">{props.hint}</div>
        )
      : null}
  </div>
);
