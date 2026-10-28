import { redirect } from 'next/navigation';

/** Edge nodes live on the providers page now; keep the old link working. */
export default function EdgePage() {
  redirect('/dashboard/providers');
}
