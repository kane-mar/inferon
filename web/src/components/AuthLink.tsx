import type { ReactNode } from 'react';

/**
 * Intentional full-page link that leaves the Next app for the Rust OAuth route
 * (`/auth/*` is served by the proxy, not by Next).
 */
export const AuthLink = (props: {
  className?: string;
  children: ReactNode;
}) => (
  // eslint-disable-next-line next/no-html-link-for-pages -- /auth/* is served by the Rust proxy, not Next
  <a href="/auth/login" className={props.className} rel="external">
    {props.children}
  </a>
);
