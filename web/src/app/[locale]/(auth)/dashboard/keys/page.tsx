import { redirect } from 'next/navigation';

/** Consolidated into Settings. The URL stays valid for old links. */
export default function SettingsRedirect() {
  redirect('/dashboard/settings');
}
