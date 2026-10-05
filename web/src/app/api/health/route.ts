/** Liveness probe for the web container (bypasses i18n middleware). */
export const dynamic = 'force-dynamic';

export function GET() {
  return Response.json({ status: 'ok' });
}
