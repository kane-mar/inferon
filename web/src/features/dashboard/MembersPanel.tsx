// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

'use client';

import type { MembersResponse } from '@/libs/InferonApi';
import { useTranslations } from 'next-intl';
import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { errorMessage } from '@/libs/errors';
import { InferonApi } from '@/libs/InferonApi';

export const MembersPanel = (props: { data: MembersResponse }) => {
  const t = useTranslations('MembersPage');
  const router = useRouter();
  const [login, setLogin] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const isOwner = props.data.role === 'owner';

  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
      setLogin('');
      router.refresh();
    } catch (cause) {
      setError(errorMessage(cause, t('error')));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-6">
      {isOwner
        ? (
            <form
              onSubmit={(event) => {
                event.preventDefault();
                void run(() => InferonApi.sameOrigin().inviteMember(login));
              }}
              className="flex flex-wrap items-end gap-3"
            >
              <label className="flex flex-col gap-1 text-sm">
                <span className="text-muted-foreground">{t('github_login')}</span>
                <input
                  className="rounded-md border bg-background px-3 py-2"
                  value={login}
                  onChange={event => setLogin(event.target.value)}
                  placeholder={t('invite_placeholder')}
                  required
                />
              </label>

              <button
                type="submit"
                disabled={busy}
                className="
                  rounded-sm bg-primary px-3 py-2 text-sm font-medium
                  text-primary-foreground
                  disabled:opacity-50
                "
              >
                {t('invite')}
              </button>
            </form>
          )
        : null}

      {error ? <p className="text-sm text-red-600">{error}</p> : null}

      <div className="overflow-x-auto">
        <table className="w-full text-left text-sm">
          <thead>
            <tr className="border-b text-muted-foreground">
              <th className="py-2 font-medium">{t('github_login')}</th>
              <th className="py-2 font-medium">{t('role')}</th>
              {isOwner ? <th className="py-2" /> : null}
            </tr>
          </thead>
          <tbody>
            {props.data.members.map(member => (
              <tr
                key={member.id}
                className="
                  border-b
                  last:border-0
                "
              >
                <td className="py-2">{member.github_login}</td>
                <td className="py-2">
                  {member.role === 'owner' ? t('owner') : t('member')}
                </td>
                {isOwner
                  ? (
                      <td className="py-2 text-right">
                        {member.role === 'owner'
                          ? null
                          : (
                              <button
                                type="button"
                                disabled={busy}
                                onClick={() =>
                                  run(() => InferonApi.sameOrigin().removeMember(member.id))}
                                className="
                                  text-sm text-red-600
                                  hover:underline
                                  disabled:opacity-50
                                "
                              >
                                {t('remove')}
                              </button>
                            )}
                      </td>
                    )
                  : null}
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div>
        <h3 className="text-sm font-medium text-muted-foreground">
          {t('pending_invites')}
        </h3>
        {props.data.invites.length === 0
          ? (
              <p className="mt-1 text-sm text-muted-foreground">{t('no_invites')}</p>
            )
          : (
              <ul className="mt-1 space-y-1 text-sm">
                {props.data.invites.map(invite => (
                  <li key={invite}>{invite}</li>
                ))}
              </ul>
            )}
      </div>
    </div>
  );
};
