'use client';

import type { ProviderModel } from '@/libs/InferonApi';
import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { errorMessage } from '@/libs/errors';
import { formatUsd, InferonApi } from '@/libs/InferonApi';

const formatContext = (tokens: number): string =>
  tokens >= 1_000_000 ? `${tokens / 1_000_000}M` : `${Math.round(tokens / 1000)}K`;

/**
 * ITEM-044 (reworked): the catalog models a provider can serve, with this
 * provider's enable/disable + default flags. Rendered inside a provider row.
 */
export const ProviderModels = (props: {
  providerId: string;
  models: ProviderModel[];
  canEdit: boolean;
}) => {
  const t = useTranslations('ProvidersPage');
  const router = useRouter();
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  if (props.models.length === 0) {
    return (
      <p className="
        border-t bg-muted/30 px-4 py-3 text-sm text-muted-foreground
      "
      >
        {t('no_models')}
      </p>
    );
  }

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

  return (
    <div className="border-t bg-muted/30 px-4 py-3">
      {error && <p className="mb-2 text-sm text-destructive">{error}</p>}
      <ul className="space-y-2">
        {props.models.map(model => (
          <li
            key={model.id}
            className="flex flex-wrap items-center gap-3 text-sm"
          >
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className="font-medium">{model.name}</span>
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
              <div className="truncate font-mono text-xs text-muted-foreground">
                {model.id}
              </div>
            </div>

            <div className="text-right font-mono text-xs text-muted-foreground">
              <div>{`${formatContext(model.context_window)} ${t('context')}`}</div>
              <div>
                {`${formatUsd(model.price.input_usd_per_million)} in · ${formatUsd(model.price.output_usd_per_million)} out / 1M`}
              </div>
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
