# A sign-in loop, or being signed out instantly

You sign in with GitHub, land on the dashboard, and are sent straight back to sign in.
Or you sign in and every page redirects to `/auth/login`.

## Symptom

`GET /dashboard` → `307 /en/dashboard` → `307 /auth/login` → GitHub → back to sign-in.
No error message, because none of these steps is an error.

## What it means

Three causes:

1. **`SESSION_SECRET` changed** (or differs between containers). The session cookie is
   signed with it; a new secret invalidates every existing cookie. Everyone is affected, at
   once, right after a deploy.
2. **The cookie is not being set or not being sent back.** `Secure` cookies need HTTPS; a
   domain mismatch (`inferon.example.com` versus `www.inferon.example.com`) drops the
   cookie; a proxy that strips cookies does the same.
3. **OAuth callback URL mismatch.** GitHub redirects to the callback you registered; if the
   app expects a different host or scheme, the code exchange fails and you land back at
   sign-in.

## Fix

1. **Check `SESSION_SECRET` is set and identical** on every container that reads the
   cookie : one value, no trailing whitespace, no quotes in the middle. Rotating it signs
   everyone out; that is expected, once.
2. **Check the cookie in the browser**: `inferon_session` present, `Secure` when on HTTPS,
   `Path=/`, and the cookie's domain matching the address bar exactly.
3. **Check the callback URL** in the GitHub OAuth app against
   `${NEXT_PUBLIC_APP_URL}/auth/callback` : host *and* scheme.
4. **Behind your own proxy**, forward `Cookie` and `Set-Cookie`. A reverse proxy that drops
   them produces exactly this loop.

## How to confirm it worked

- One sign-in lands on `/dashboard` and stays there across three page loads.
- `/auth/me` returns your user with `200` while signed in, and `401` after signing out.
- Signing out (`POST /auth/logout`) ends the session and a reload does not restore it.

## When to stop and ask

- One browser loops and another does not : a cookie or extension problem on that browser.
- Sign-in loops only for users who arrived after a deploy: the secret changed.
- Everything works over `http://` and not over `https://` , a `Secure` cookie being set on a
  plain-HTTP page, or the reverse.

_Last verified 2026-11-06 against the live host (master acdaffc)._
