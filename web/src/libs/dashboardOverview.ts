// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import type { PayloadsReport, Summary } from './InferonApi';
import { metricTone, THRESHOLDS } from './reportTone';

/** One thing worth looking at now, and where to fix it. */
export type Attention = {
  kind: 'errors' | 'latency' | 'waste' | 'lint';
  /** Higher is more urgent. Errors are a failure; the rest are money. */
  severity: number;
  /** Dollar impact, so two findings of the same severity can be ordered. */
  amount: number;
  href: string;
};

/**
 * What deserves attention on the landing page (ITEM-104).
 *
 * Only things that are actionable *now*: an error rate or a p95 outside the
 * attention line, and waste the audit or the lint has already priced. A healthy
 * tenant gets an empty list, so the strip can be absent rather than green.
 */
export const attention = (input: {
  summary: Summary;
  waste?: Record<string, { requests: number; savings_est_usd: string }> | null;
  payloads?: PayloadsReport | null;
}): Attention[] => {
  const { summary, waste, payloads } = input;
  const findings: Attention[] = [];

  const errors = summary.requests > 0 ? summary.errors / summary.requests : null;
  if (metricTone(errors, THRESHOLDS.errorRate) === 'bad') {
    findings.push({ kind: 'errors', severity: 0, amount: 0, href: '/dashboard/routing' });
  }
  if (metricTone(summary.ttft_p95_ms, THRESHOLDS.ttftP95Ms) === 'bad') {
    findings.push({ kind: 'latency', severity: 1, amount: 0, href: '/dashboard/routing' });
  }

  const recoverable = Object.values(waste ?? {}).reduce(
    (total, row) => total + Number(row.savings_est_usd),
    0,
  );
  if (recoverable > 0) {
    findings.push({ kind: 'waste', severity: 1, amount: recoverable, href: '/dashboard/routing' });
  }

  const linted = Number(payloads?.lint.preflight.wasted_usd ?? 0);
  if ((payloads?.lint.preflight.findings ?? 0) > 0 && linted > 0) {
    findings.push({ kind: 'lint', severity: 1, amount: linted, href: '/dashboard/payloads' });
  }

  return findings.sort((a, b) => a.severity - b.severity || b.amount - a.amount);
};
