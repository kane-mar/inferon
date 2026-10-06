import { ArrowRightIcon } from '@radix-ui/react-icons';
import { useTranslations } from 'next-intl';
import { AuthLink } from '@/components/AuthLink';
import { buttonVariants } from '@/components/ui/buttonVariants';
import { Section } from '@/features/shell/Section';

export const CTA = () => {
  const t = useTranslations('CTA');

  return (
    <Section>
      <div className="
        rounded-lg bg-accent px-6 py-12 text-center text-accent-foreground
      "
      >
        <div className="
          text-3xl font-semibold tracking-tight
          md:text-4xl
        "
        >
          {t('title')}
        </div>

        <div className="mt-2 text-lg font-medium opacity-80">
          {t('description')}
        </div>

        <div className="mt-6">
          <AuthLink
            className={buttonVariants({ size: 'lg', className: 'whitespace-pre-line' })}
          >
            {t('button_text')}

            <ArrowRightIcon className="ml-1 size-5" />
          </AuthLink>
        </div>
      </div>
    </Section>
  );
};
