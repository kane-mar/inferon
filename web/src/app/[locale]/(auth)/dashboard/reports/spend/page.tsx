import { redirect } from 'next/navigation';

/** The spend report moved to the dashboard's Spend tab. */
export default function SpendRedirect() {
  redirect('/dashboard/spend');
}
