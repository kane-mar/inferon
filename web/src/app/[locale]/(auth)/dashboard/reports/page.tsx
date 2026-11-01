import { redirect } from 'next/navigation';

/**
 * The Reports section is gone: its pages are tabs under the dashboard
 * (`dashboardNav`). The URL stays valid as a redirect so old links keep working.
 */
export default function ReportsRedirect() {
  redirect('/dashboard');
}
