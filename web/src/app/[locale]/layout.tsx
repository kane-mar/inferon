// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import type { Metadata, Viewport } from 'next';
import { hasLocale, NextIntlClientProvider } from 'next-intl';
import { setRequestLocale } from 'next-intl/server';
import { Inter, JetBrains_Mono } from 'next/font/google';
import { notFound } from 'next/navigation';
import { routing } from '@/libs/I18nRouting';
import '@/styles/global.css';

// Self-hosted at build time (no runtime CDN) — the design system's two faces:
// Inter for UI, JetBrains Mono for labels/code (see styles/global.css).
const inter = Inter({
  subsets: ['latin'],
  variable: '--font-inter',
  display: 'swap',
});

const jetbrainsMono = JetBrains_Mono({
  subsets: ['latin'],
  variable: '--font-jetbrains-mono',
  display: 'swap',
});

/**
 * ITEM-143: `themeColor` lives on the viewport export in this Next version; on
 * `metadata` it is silently ignored, which is why the first attempt produced no
 * tag at all.
 */
export const metadata: Metadata = {
  icons: [
    {
      rel: 'apple-touch-icon',
      url: '/apple-touch-icon.png',
    },
    {
      rel: 'icon',
      type: 'image/png',
      sizes: '32x32',
      url: '/favicon-32x32.png',
    },
    {
      rel: 'icon',
      type: 'image/png',
      sizes: '16x16',
      url: '/favicon-16x16.png',
    },
    {
      rel: 'icon',
      url: '/favicon.ico',
    },
  ],
};

export const viewport: Viewport = {
  width: 'device-width',
  initialScale: 1,
  // ITEM-143: the browser's own widgets (scrollbars, form controls) follow the
  // theme; without this dark mode shows white scrollbars.
  themeColor: [
    { media: '(prefers-color-scheme: light)', color: '#fcfdff' },
    { media: '(prefers-color-scheme: dark)', color: '#15171c' },
  ],
};

export function generateStaticParams() {
  return routing.locales.map(locale => ({ locale }));
}

export default async function RootLayout(props: {
  children: React.ReactNode;
  params: Promise<{ locale: string }>;
}) {
  const { locale } = await props.params;

  if (!hasLocale(routing.locales, locale)) {
    notFound();
  }

  setRequestLocale(locale);

  return (
    <html
      lang={locale}
      className={`
        ${inter.variable}
        ${jetbrainsMono.variable}
      `}
      // `color-scheme` tells the browser which palette its own widgets use.
      style={{ colorScheme: 'light dark' }}
      suppressHydrationWarning
    >
      <body>
        <NextIntlClientProvider>
          {props.children}
        </NextIntlClientProvider>
      </body>
    </html>
  );
}
