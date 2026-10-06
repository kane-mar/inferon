import type { Metadata } from 'next';
import { getTranslations, setRequestLocale } from 'next-intl/server';
import { CTA } from '@/features/landing/CTA';
import { Features } from '@/features/landing/Features';
import { Hero } from '@/features/landing/Hero';
import { LandingFooter } from '@/features/landing/LandingFooter';
import { LandingHeader } from '@/features/landing/LandingHeader';

type IndexProps = {
  params: Promise<{ locale: string }>;
};

export async function generateMetadata(props: IndexProps): Promise<Metadata> {
  const { locale } = await props.params;
  const t = await getTranslations({
    locale,
    namespace: 'Index',
  });

  return {
    title: t('meta_title'),
    description: t('meta_description'),
  };
}

export default async function Index(props: IndexProps) {
  const { locale } = await props.params;
  setRequestLocale(locale);

  return (
    <>
      <LandingHeader />
      <Hero />
      <Features />
      <CTA />
      <LandingFooter />
    </>
  );
}
