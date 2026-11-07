// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { createNavigation } from 'next-intl/navigation';
import { routing } from './I18nRouting';

export const { Link, usePathname, useRouter } = createNavigation(routing);
