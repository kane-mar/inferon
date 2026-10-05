import { useTranslations } from 'next-intl';
import { AuthLink } from '@/components/AuthLink';
import { CenteredFooter } from '@/features/landing/CenteredFooter';
import { Section } from '@/features/landing/Section';
import { Link } from '@/libs/I18nNavigation';
import { AppConfig } from '@/utils/AppConfig';
import { Logo } from './Logo';

const REPOSITORY = 'https://github.com/kane-mar/inferon';

export const Footer = () => {
  const t = useTranslations('Footer');

  return (
    <Section className="pt-0 pb-16">
      <CenteredFooter
        logo={<Logo />}
        name={AppConfig.name}
        iconList={<></>}
        legalLinks={<></>}
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
      </CenteredFooter>
    </Section>
  );
};
