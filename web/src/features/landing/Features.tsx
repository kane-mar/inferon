// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { useTranslations } from 'next-intl';
import { Section } from '@/features/shell/Section';

const CubeIcon = () => (
  <svg
    className="stroke-primary-foreground stroke-2"
    xmlns="http://www.w3.org/2000/svg"
    viewBox="0 0 24 24"
    fill="none"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <path d="M0 0h24v24H0z" stroke="none" />
    <path d="M12 3l8 4.5v9L12 21l-8-4.5v-9L12 3M12 12l8-4.5M12 12v9M12 12L4 7.5" />
  </svg>
);

const FEATURES = [
  ['feature1_title', 'feature1_description'],
  ['feature2_title', 'feature2_description'],
  ['feature3_title', 'feature3_description'],
  ['feature4_title', 'feature4_description'],
  ['feature5_title', 'feature5_description'],
  ['feature6_title', 'feature6_description'],
] as const;

export const Features = () => {
  const t = useTranslations('Features');

  return (
    <div id="features" className="border-y bg-secondary">
      <Section
        subtitle={t('section_subtitle')}
        title={t('section_title')}
      >
        <div className="
          grid grid-cols-1 gap-x-6 gap-y-8
          md:grid-cols-3
        "
        >
          {FEATURES.map(([title, description]) => (
            <div
              key={title}
              className="
                group rounded-lg border border-border bg-card p-5
                transition-colors
                hover:border-accent
              "
            >
              <div className="
                size-12 rounded-md bg-accent/15 p-2 text-accent-ink
                [&_svg]:stroke-white [&_svg]:stroke-2
              "
              >
                <CubeIcon />
              </div>

              <div className="mt-3 text-sm font-semibold">
                {t(title)}
              </div>

              <div className="my-3 w-8 border-t border-accent" />

              <div className="mt-2 text-muted-foreground">{t(description)}</div>
            </div>
          ))}
        </div>
      </Section>
    </div>
  );
};
