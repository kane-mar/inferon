// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { defineRouting } from 'next-intl/routing';
import { AllLocales, AppConfig } from '@/utils/AppConfig';

export const routing = defineRouting({
  locales: AllLocales,
  localePrefix: AppConfig.i18n.localePrefix,
  defaultLocale: AppConfig.i18n.defaultLocale,
});
