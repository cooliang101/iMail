import type { ApiGatewayCredential } from '../../app-model';
import type { DeveloperToken } from '../../types';

export function resolveActiveApiCredential(latest: ApiGatewayCredential | undefined, tokens: DeveloperToken[], now = Date.now()) {
  if (!latest) return undefined;
  const listed = tokens.find((token) => token.id === latest.detail.id);
  if (!listed || Date.parse(listed.expiresAt) <= now) return undefined;
  return latest;
}

export function buildGatewayMessageUrl(endpoint: string, mailbox: string) {
  return `${endpoint}/mailboxes/${encodeURIComponent(mailbox)}/messages?limit=10`;
}

export function buildGatewayCurl(endpoint: string, mailbox: string, rawToken: string) {
  return [
    `curl "${buildGatewayMessageUrl(endpoint, mailbox)}"`,
    `  -H "Authorization: Bearer ${rawToken}"`,
  ].join(' \\\n');
}

export function buildAgentGatewayContext({ endpoint, mailbox, rawToken, authorizedMailboxes }: {
  endpoint: string;
  mailbox: string;
  rawToken: string;
  authorizedMailboxes: string[];
}) {
  return [
    '请使用以下 iMail 本地 REST API 网关调用邮件。',
    `Base URL: ${endpoint}`,
    `OpenAPI: ${endpoint.replace(/\/gateway\/v1$/, '/gateway/openapi.json')}`,
    `Authorization: Bearer ${rawToken}`,
    `当前邮箱: ${mailbox}`,
    `Token 授权邮箱: ${authorizedMailboxes.join(', ')}`,
    '',
    '读取最新 10 封邮件：',
    buildGatewayCurl(endpoint, mailbox, rawToken),
  ].join('\n');
}
