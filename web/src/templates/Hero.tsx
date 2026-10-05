import { ArrowRightIcon, GitHubLogoIcon } from '@radix-ui/react-icons';
import { useTranslations } from 'next-intl';
import { AuthLink } from '@/components/AuthLink';
import { badgeVariants } from '@/components/ui/badgeVariants';
import { buttonVariants } from '@/components/ui/buttonVariants';
import { CenteredHero } from '@/features/landing/CenteredHero';
import { Section } from '@/features/landing/Section';

const REPOSITORY = 'https://github.com/kane-mar/inferon';

/** The drop-in claim, made literal. Static, so the hero stays a server component. */
const SNIPPET = `from openai import OpenAI

client = OpenAI(
    base_url="https://inferon.agilefederation.com/v1",
    api_key="sk-inferon-\u2026",
)

client.chat.completions.create(
    model="deepseek-chat",
    messages=[{"role": "user", "content": "Say hi"}],
)`;

export const Hero = () => {
  const t = useTranslations('Hero');

  return (
    <Section className="
      px-3 pt-16 pb-20
      md:pt-28
    "
    >
      <CenteredHero
        banner={(
          <span
            className={badgeVariants({
              variant: 'outline',
              className: 'font-mono text-xs tracking-wide uppercase',
            })}
          >
            {t('badge')}
          </span>
        )}
        title={t.rich('title', {
          important: chunks => (
            <span className="
              bg-linear-to-r from-accent to-chart-2 bg-clip-text
              text-transparent
            "
            >
              {chunks}
            </span>
          ),
        })}
        description={t('description')}
        buttons={(
          <>
            <AuthLink className={buttonVariants({ size: 'lg' })}>
              {t('primary_button')}
              <ArrowRightIcon className="ml-1 size-5" />
            </AuthLink>

            <a
              className={buttonVariants({ variant: 'outline', size: 'lg' })}
              href={REPOSITORY}
            >
              <GitHubLogoIcon className="mr-2 size-5" />
              {t('secondary_button')}
            </a>
          </>
        )}
      />

      <div className="
        mx-auto mt-16 max-w-2xl overflow-hidden rounded-lg border bg-[#1e1e1e]
        shadow-sm
      "
      >
        <div className="
          flex items-center gap-1.5 border-b border-white/10 px-4 py-2.5
        "
        >
          <span className="size-2.5 rounded-full bg-white/20" />
          <span className="size-2.5 rounded-full bg-white/20" />
          <span className="size-2.5 rounded-full bg-white/20" />
          <span className="ml-2 font-mono text-xs text-white/40">python</span>
        </div>

        <pre className="
          overflow-x-auto p-5 font-mono text-sm/relaxed text-white/90
        "
        >
          <code>{SNIPPET}</code>
        </pre>
      </div>
    </Section>
  );
};
