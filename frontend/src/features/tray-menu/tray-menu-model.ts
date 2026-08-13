export function visibleTrayAccounts<T>(accounts: T[], expanded: boolean, limit = 5) {
  return expanded ? accounts : accounts.slice(0, limit);
}
