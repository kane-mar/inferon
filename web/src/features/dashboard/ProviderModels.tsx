'use client';

import type { ProviderModel } from '@/libs/InferonApi';
import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useEffect, useState } from 'react';
import { Button } from '@/components/ui/button';
import { errorMessage } from '@/libs/errors';
import { formatUsd, InferonApi } from '@/libs/InferonApi';

/**
 * ITEM-044 (live): the models a provider's host advertises, discovered from
 * `GET <base>/models` when this section opens, with enable/disable + default.
 */
export const ProviderModels = (props: { providerId: string; canEdit: boolean }) => {
  const t = useTranslations('ProvidersPage');
  const router = useRouter();
  const [models, setModels] = useState<ProviderModel[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    InferonApi.sameOrigin()
      .providerModels(props.providerId)
      .then((data) => {
        if (active) {
          setModels(data.models);
        }
      })
      .catch((cause) => {
        if (active) {
          setError(errorMessage(cause, t('models_error')));
        }
      });
    return () => {
      active = false;
    };
  }, [props.providerId, t]);

  const update = async (
    modelId: string,
    body: { enabled?: boolean; is_default?: boolean },
  ) => {
    setBusy(modelId);
    setError(null);
    try {
      await InferonApi.sameOrigin().updateProviderModel(props.providerId, modelId, body);
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('error')));
    } finally {
      setBusy(null);
    }
  };

  if (error) {
    return (
      <p className="border-t bg-muted/30 px-4 py-3 text-sm text-destructive">{error}</p>
    );
  }
  if (models === null) {
    return (
      <p className="
        border-t bg-muted/30 px-4 py-3 text-sm text-muted-foreground
      "
      >
        {t('loading')}
      </p>
    );
  }
  if (models.length === 0) {
    return (
      <p className="
        border-t bg-muted/30 px-4 py-3 text-sm text-muted-foreground
      "
      >
        {t('no_models')}
      </p>
    );
  }

  return (
    <div className="border-t bg-muted/30 px-4 py-3">
      <ul className="space-y-2">
        {models.map(model => (
          <li
            key={model.id}
            className="flex flex-wrap items-center gap-3 text-sm"
          >
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className="truncate font-mono text-xs">{model.id}</span>
                {model.is_default && (
                  <span className="
                    rounded-sm bg-accent px-1.5 py-0.5 font-mono text-xs
                    text-accent-foreground
                  "
                  >
                    {t('default')}
                  </span>
                )}
                {!model.enabled && (
                  <span className="
                    rounded-sm bg-secondary px-1.5 py-0.5 font-mono text-xs
                    text-muted-foreground
                  "
                  >
                    {t('disabled')}
                  </span>
                )}
              </div>
            </div>

            <div className="text-right font-mono text-xs text-muted-foreground">
              {model.price.local
                ? t('local_free')
                : model.price.priced
                  ? `${formatUsd(model.price.input_usd_per_million)} in · ${formatUsd(model.price.output_usd_per_million)} out / 1M`
                  : t('unpriced')}
            </div>

            {props.canEdit && (
              <div className="flex items-center gap-1">
                <Button
                  variant="ghost"
                  size="sm"
                  disabled={busy === model.id}
                  onClick={() => update(model.id, { enabled: !model.enabled })}
                >
                  {model.enabled ? t('disable') : t('enable')}
                </Button>
                {model.enabled && !model.is_default && (
                  <Button
                    variant="ghost"
                    size="sm"
                    disabled={busy === model.id}
                    onClick={() => update(model.id, { is_default: true })}
                  >
                    {t('make_default')}
                  </Button>
                )}
              </div>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
};
