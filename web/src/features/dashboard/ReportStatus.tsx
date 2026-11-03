/** A met / not-met marker; `null` means there were too few samples to say. */
export const Status = (props: { ok: boolean | null; met: string; missed: string }) => {
  if (props.ok == null) {
    return <span className="text-sm text-muted-foreground">—</span>;
  }
  return (
    <span
      className={props.ok
        ? `
          text-sm font-medium text-emerald-700
          dark:text-emerald-400
        `
        : 'text-sm font-medium text-destructive'}
    >
      {props.ok ? props.met : props.missed}
    </span>
  );
};
