export const OAUTH_WAIT_TIMEOUT_MS = 10 * 60_000;
export const OAUTH_POPUP_CHECK_INTERVAL_MS = 500;
export const OAUTH_STATUS_POLL_INTERVAL_MS = 1_000;

export function oauthWaitExpired(startedAt: number, now = Date.now()) {
  return now - startedAt > OAUTH_WAIT_TIMEOUT_MS;
}

export function waitForOAuthStatus() {
  return new Promise<void>((resolve) => window.setTimeout(resolve, OAUTH_STATUS_POLL_INTERVAL_MS));
}

export function openOAuthPopup() {
  const popup = window.open('', 'imail-oauth', 'popup,width=560,height=720,menubar=no,toolbar=no');
  if (!popup) return null;
  popup.document.write('<title>iMail</title><p style="font-family:system-ui;padding:32px">正在打开安全登录…</p>');
  return popup;
}
