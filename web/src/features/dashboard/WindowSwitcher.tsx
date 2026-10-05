import { Link } from '@/libs/I18nNavigation';

const WINDOWS = [7, 30] as const;

export const WindowSwitcher = (props: { days: number; path: string }) => (
  <div className="flex gap-1 text-sm">
    {WINDOWS.map(days => (
      <Link
        key={days}
        href={{ pathname: props.path, query: { days } }}
        className={
          days === props.days
            ? 'rounded-sm border bg-primary px-2 py-1 text-primary-foreground'
            : `
              rounded-sm border bg-background px-2 py-1
              hover:bg-muted
            `
        }
      >
        {`${days}d`}
      </Link>
    ))}
  </div>
);
