/** `1234` → `1,234` — grouping on the integer part only. */
const grouped = (whole: string): string =>
  whole.replace(/\B(?=(\d{3})+(?!\d))/g, ',');

/** What the API sends: a decimal string, optionally negative. */
const DECIMAL = /^-?\d*(?:\.\d*)?$/;

/**
 * Money at the precision that matters (ITEM-106): two decimals from a cent up,
 * up to six below it, and never a run of trailing zeros.
 *
 * A dashboard that prints `$0.00037708` on every row is noise; one that prints
 * `$0.00` for a real cost is a lie. This is presentation only — the exact
 * decimal string is what the API returns and `formatUsd` still prints verbatim.
 */
export const money = (value: string | number | null | undefined): string => {
  if (value == null) {
    return '—';
  }
  const text = typeof value === 'number'
    ? (Number.isFinite(value) ? value.toFixed(6) : '')
    : value.trim();
  if (text === '' || !DECIMAL.test(text)) {
    return '—';
  }

  const negative = text.startsWith('-');
  const unsigned = negative ? text.slice(1) : text;
  const [rawWhole = '0', rawFraction = ''] = unsigned.split('.');
  const whole = rawWhole === '' ? '0' : rawWhole;
  const amount = Number(`${whole}.${rawFraction || '0'}`);

  if (amount >= 0.01) {
    const [w, f] = amount.toFixed(2).split('.');
    return `${negative ? '-' : ''}$${grouped(w!)}.${f}`;
  }
  if (amount === 0) {
    return '$0.00';
  }

  // Below a cent: the six decimals the API sends, minus trailing zeros.
  const fraction = rawFraction.slice(0, 6).replace(/0+$/, '');
  if (fraction === '') {
    return `${negative ? '-' : ''}<$0.000001`;
  }
  return `${negative ? '-' : ''}$${grouped(whole)}.${fraction}`;
};
