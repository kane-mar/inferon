// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { ArrowRightIcon, GitHubLogoIcon } from '@radix-ui/react-icons';
import { useTranslations } from 'next-intl';
import { AuthLink } from '@/components/AuthLink';
import { badgeVariants } from '@/components/ui/badgeVariants';
import { buttonVariants } from '@/components/ui/buttonVariants';
import { Section } from '@/features/shell/Section';

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
      <div className="flex justify-center">
        <span
          className={badgeVariants({
            variant: 'outline',
            className: 'text-xs font-semibold tracking-wide uppercase',
          })}
        >
          {t('badge')}
        </span>
      </div>

      <h1 className="
        mx-auto mt-6 max-w-4xl text-center text-4xl font-semibold
        tracking-tighter text-balance
        sm:text-5xl
        md:text-7xl
      "
      >
        {t.rich('title', {
          important: chunks => (
            <span className="text-accent-ink">
              {chunks}
            </span>
          ),
        })}
      </h1>

      <p className="
        mx-auto mt-6 max-w-2xl text-center text-lg text-pretty
        text-muted-foreground
        md:text-xl
      "
      >
        {t('description')}
      </p>

      <div className="
        mt-10 flex flex-wrap justify-center gap-3
        max-sm:flex-col max-sm:items-center
      "
      >
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
      </div>

      <div className="
        mx-auto mt-16 max-w-2xl overflow-hidden rounded-lg border bg-[#1e1e1e]
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
