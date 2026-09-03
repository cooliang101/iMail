export const COLLAPSED_TRAY_ACCOUNT_LIMIT = 5;

export function visibleTrayAccounts<T>(accounts: T[], expanded: boolean, limit = COLLAPSED_TRAY_ACCOUNT_LIMIT) {
  return expanded ? accounts : accounts.slice(0, limit);
}
