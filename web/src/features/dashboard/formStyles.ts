// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/** The shared dashboard form-input style (single source so panels stay in step). */
export const FIELD_INPUT = `
  mt-1 w-full rounded-md border bg-background px-3 py-2 text-sm outline-none
  focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50
`;

export const MONO_FIELD_INPUT = `${FIELD_INPUT} font-mono`;
