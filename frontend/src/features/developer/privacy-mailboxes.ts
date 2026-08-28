import type { AppleHmeAddress } from '../../app-model';
import { api } from '../../services';
import type { Account } from '../../types';

export type PrivacyMailbox = { address: AppleHmeAddress; ownerEmail: string };
type PrivacyMailboxRequest = (path: string) => Promise<{ addresses: AppleHmeAddress[] }>;

export async function loadPrivacyMailboxes(accounts: Account[], request: PrivacyMailboxRequest = api) {
  const results = await Promise.all(accounts.filter((account) => account.provider === 'icloud').map(async (account) => {
    try {
      const response = await request(`/api/accounts/${account.id}/apple-hme/addresses`);
      return {
        mailboxes: response.addresses.map((address) => ({ address, ownerEmail: account.email })),
        failed: false,
      };
    } catch {
      return { mailboxes: [] as PrivacyMailbox[], failed: true };
    }
  }));

  return {
    mailboxes: results.flatMap((result) => result.mailboxes),
    failedCount: results.filter((result) => result.failed).length,
  };
}
