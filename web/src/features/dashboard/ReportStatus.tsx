// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/** A met / not-met marker; `null` means there were too few samples to say. */
export const Status = (props: { ok: boolean | null; met: string; missed: string }) => {
  if (props.ok == null) {
    return <span className="text-sm text-muted-foreground">—</span>;
  }
  return (
    <span
      className={props.ok
        ? 'text-sm font-medium text-success'
        : 'text-sm font-medium text-destructive'}
    >
      {props.ok ? props.met : props.missed}
    </span>
  );
};
