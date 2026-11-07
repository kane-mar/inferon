// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-react';
import { page } from 'vitest/browser';
import { Logo } from './Logo';

describe('Logo', () => {
  it('renders the Inferon wordmark', async () => {
    await render(<Logo />);

    expect(page.getByText('Inferon')).toBeInTheDocument();
  });
});
