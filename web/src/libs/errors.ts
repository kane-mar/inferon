import { InferonApiError } from './InferonApi';

/** The API's own message for an `InferonApiError`; the caller's `fallback` otherwise. */
export const errorMessage = (error: unknown, fallback: string): string =>
  error instanceof InferonApiError ? error.message : fallback;
