'use client';

import type { ModelsResponse } from '@/libs/InferonApi';
import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { errorMessage } from '@/libs/errors';
import { formatUsd, InferonApi } from '@/libs/InferonApi';

const formatContext = (tokens: number): string =>
  tokens >= 1_000_000 ? `${tokens / 1_000_000}M` : `${Math.round(tokens / 1000)}K`;

export const ModelsPanel = (props: { data: ModelsResponse }) => {
  const t = useTranslations('ModelsPage');
  const router = useRouter();
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const canEdit = props.data.role === 'owner';

  const update = async (
    modelId: string,
    body: { enabled?: boolean; is_default?: boolean },
  ) => {
    setBusy(modelId);
    setError(null);
    try {
      await InferonApi.sameOrigin().updateModel(modelId, body);
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('error')));
    } finally {
      setBusy(null);
    }
  };

  if (props.data.models.length === 0) {
    return <p className="text-sm text-muted-foreground">{t('empty')}</p>;
  }

  const kinds = [...new Set(props.data.models.map(model => model.kind))];

  return (
    <div className="space-y-6">
      {error && <p className="text-sm text-destructive">{error}</p>}

      {kinds.map(kind => (
        <div key={kind}>
          <h3 className="
            mb-2 font-mono text-xs font-medium tracking-wide
            text-muted-foreground uppercase
          "
          >
            {kind}
          </h3>
          <ul className="overflow-hidden rounded-lg border bg-card">
            {props.data.models
              .filter(model => model.kind === kind)
              .map(model => (
                <li
                  key={model.id}
                  className="
                    flex flex-wrap items-center gap-3 border-b px-4 py-3 text-sm
                    last:border-0
                  "
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
                    </div>
                    <div className="
                      truncate font-mono text-xs text-muted-foreground
                    "
                    >
                      {model.id}
                    </div>
                  </div>

                  <div className="
                    text-right font-mono text-xs text-muted-foreground
                  "
                  >
                    <div>{`${formatContext(model.context_window)} ${t('context')}`}</div>
                    <div>
                      {`${formatUsd(model.price.input_usd_per_million)} in · ${formatUsd(model.price.output_usd_per_million)} out / 1M`}
                    </div>
                  </div>

                  <div className="flex gap-1 font-mono text-xs">
                    {model.reasoning && (
                      <span className="rounded-sm bg-secondary px-1.5 py-0.5">
                        {t('reasoning')}
                      </span>
                    )}
                    {model.images && (
                      <span className="rounded-sm bg-secondary px-1.5 py-0.5">
                        {t('images')}
                      </span>
                    )}
                  </div>

                  {canEdit && (
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
      ))}
    </div>
  );
};
