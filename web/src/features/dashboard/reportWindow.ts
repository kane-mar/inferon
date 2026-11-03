/** The window every report offers: `?days=7|30|90`, defaulting to 30. */
export const WINDOWS = [7, 30, 90] as const;

export const DEFAULT_DAYS = 30;

export const reportDays = (
  params: { days?: string } | undefined,
  fallback = DEFAULT_DAYS,
): number => {
  const requested = Number(params?.days);

  return (WINDOWS as readonly number[]).includes(requested) ? requested : fallback;
};
