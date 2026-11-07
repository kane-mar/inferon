// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { useTranslations } from 'next-intl';
import { AuthLink } from '@/components/AuthLink';
import { Logo } from '@/features/shell/Logo';
import { Section } from '@/features/shell/Section';
import { Link } from '@/libs/I18nNavigation';
import { AppConfig } from '@/utils/AppConfig';

const REPOSITORY = 'https://github.com/kane-mar/inferon';

export const LandingFooter = () => {
  const t = useTranslations('Footer');

  return (
    <Section className="pt-0 pb-16">
      <div className="flex flex-col items-center text-center">
        <Logo />

        <ul className="
          mt-4 flex gap-x-8 text-sm
          max-sm:flex-col
          [&_a:hover]:opacity-70
        "
        >
          <li>
            <AuthLink>{t('sign_in')}</AuthLink>
          </li>

          <li>
            <Link href="/dashboard">{t('dashboard')}</Link>
          </li>

          <li>
            <a href={REPOSITORY}>{t('source')}</a>
          </li>
        </ul>

        <div className="
          mt-6 flex w-full items-center justify-between gap-y-2 border-t pt-3
          text-sm text-muted-foreground
          max-md:flex-col
        "
        >
          {t('footer_text', {
            year: new Date().getFullYear(),
            name: AppConfig.name,
          })}
        </div>
      </div>
    </Section>
  );
};
