import { useTranslations } from 'next-intl';
import { AuthLink } from '@/components/AuthLink';
import { LocaleSwitcher } from '@/components/LocaleSwitcher';
import { buttonVariants } from '@/components/ui/buttonVariants';
import { CenteredMenu } from '@/features/landing/CenteredMenu';
import { Section } from '@/features/landing/Section';
import { Link } from '@/libs/I18nNavigation';
import { Logo } from './Logo';

const REPOSITORY = 'https://github.com/kane-mar/inferon';

export const Navbar = () => {
  const t = useTranslations('Navbar');

  return (
    <Section className="border-b px-3 py-4">
      <CenteredMenu
        logo={<Logo />}
        rightMenu={(
          <>
            <li>
              <LocaleSwitcher />
            </li>
            <li className="mr-2.5 ml-1">
              <AuthLink>{t('sign_in')}</AuthLink>
            </li>
            <li>
              <Link className={buttonVariants()} href="/dashboard">
                {t('dashboard')}
              </Link>
            </li>
          </>
        )}
      >
        <li>
          <Link href="/#features">{t('features')}</Link>
        </li>

        <li>
          <a href={REPOSITORY}>{t('source')}</a>
        </li>
      </CenteredMenu>
    </Section>
  );
};
