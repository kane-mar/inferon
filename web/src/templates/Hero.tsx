import { ArrowRightIcon, GitHubLogoIcon } from '@radix-ui/react-icons';
import { useTranslations } from 'next-intl';
import { AuthLink } from '@/components/AuthLink';
import { badgeVariants } from '@/components/ui/badgeVariants';
import { buttonVariants } from '@/components/ui/buttonVariants';
import { CenteredHero } from '@/features/landing/CenteredHero';
import { Section } from '@/features/landing/Section';

const REPOSITORY = 'https://github.com/kane-mar/inferon';

export const Hero = () => {
  const t = useTranslations('Hero');

  return (
    <Section className="py-36">
      <CenteredHero
        banner={(
          <span className={badgeVariants()}>
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
            <a
              className={buttonVariants({ variant: 'outline', size: 'lg' })}
              href={REPOSITORY}
            >
              <GitHubLogoIcon className="mr-2 size-5" />
              {t('secondary_button')}
            </a>

            <AuthLink
              className={buttonVariants({ size: 'lg' })}
            >
              {t('primary_button')}
              <ArrowRightIcon className="ml-1 size-5" />
            </AuthLink>
          </>
        )}
      />
    </Section>
  );
};
