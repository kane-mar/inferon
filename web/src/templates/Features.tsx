import { useTranslations } from 'next-intl';
import { Background } from '@/components/Background';
import { FeatureCard } from '@/features/landing/FeatureCard';
import { Section } from '@/features/landing/Section';

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
    <div id="features">
      <Background className="border-y">
        <Section
          subtitle={t('section_subtitle')}
          title={t('section_title')}
          description={t('section_description')}
        >
          <div className="
            grid grid-cols-1 gap-x-6 gap-y-8
            md:grid-cols-3
          "
          >
            {FEATURES.map(([title, description]) => (
              <FeatureCard key={title} icon={<CubeIcon />} title={t(title)}>
                {t(description)}
              </FeatureCard>
            ))}
          </div>
        </Section>
      </Background>
    </div>
  );
};
