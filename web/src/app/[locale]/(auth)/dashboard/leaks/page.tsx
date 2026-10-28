import { redirect } from 'next/navigation';

/** The leak report is now the cost audit; keep old links working. */
export default function LeaksPage() {
  redirect('/dashboard/audit');
}
