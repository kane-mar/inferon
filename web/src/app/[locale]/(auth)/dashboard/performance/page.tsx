import { redirect } from 'next/navigation';

/** The performance report split into Runtime and Payloads (ITEM-103). */
export default function PerformanceRedirect() {
  redirect('/dashboard/runtime');
}
