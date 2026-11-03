import { redirect } from 'next/navigation';

/** Folded into another report (ITEM-105); the URL stays valid. */
export default function PromptsRedirect() {
  redirect('/dashboard/datasets');
}
