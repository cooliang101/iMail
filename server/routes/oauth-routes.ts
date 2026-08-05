import { Router } from 'express';
import { z } from 'zod';
import { asyncRoute } from '../http/async-route.js';
import { oauthStartSchema } from '../http/schemas.js';
import { beginOAuth, completeOAuth, completedOAuthAccount, oauthCallbackHtml, type OAuthProviderKey } from '../oauth.js';
import { currentUserId } from '../auth/context.js';
import { recordRequestSecurityEvent, recordSecurityEvent } from '../auth/http.js';
import { publicAccount } from '../http/presenters.js';

export const oauthRouter = Router();

oauthRouter.post('/oauth/start', asyncRoute(async (req, res) => {
  const input = oauthStartSchema.parse(req.body);
  const result = await beginOAuth(input);
  recordRequestSecurityEvent(req, res, 'account.oauth-started', { provider: input.provider });
  res.json(result);
}));

oauthRouter.post('/oauth/status', asyncRoute(async (req, res) => {
  const { state } = z.object({ state: z.string().min(1) }).parse(req.body);
  const account = await completedOAuthAccount(state);
  res.json(account ? { completed: true, account: publicAccount(account) } : { completed: false });
}));

for (const providerKey of ['google', 'microsoft', 'yahoo'] as const) {
  oauthRouter.get(`/oauth/${providerKey}/callback`, asyncRoute(async (req, res) => {
    try {
      const account = await completeOAuth({
        providerKey: providerKey as OAuthProviderKey,
        state: typeof req.query.state === 'string' ? req.query.state : undefined,
        code: typeof req.query.code === 'string' ? req.query.code : undefined,
        error: typeof req.query.error === 'string' ? req.query.error : undefined,
        errorDescription: typeof req.query.error_description === 'string' ? req.query.error_description : undefined,
      });
      const warning = account.status === 'error' ? account.lastError : undefined;
      recordSecurityEvent('account.oauth-completed', req.ip || 'unknown', currentUserId(), {
        accountId: account.id,
        provider: account.provider,
        connectionReady: String(!warning),
      });
      res.type('html').send(oauthCallbackHtml({
        success: true,
        accountId: account.id,
        warning,
        message: warning ? `${account.email} 的 OAuth 授权已安全保存。邮件连接暂时失败，iMail 将保留授权供后续重试。` : `${account.email} 已通过 OAuth 安全连接。`,
      }));
    } catch (error) {
      const message = error instanceof Error ? error.message : 'OAuth 登录失败';
      const correlationId = typeof req.query.correlation_id === 'string' ? req.query.correlation_id : undefined;
      const traceId = typeof req.query.trace_id === 'string' ? req.query.trace_id : undefined;
      console.error(`[OAuth ${providerKey}] ${message}`, { correlationId, traceId, error });
      res.status(400).type('html').send(oauthCallbackHtml({ success: false, message }));
    }
  }));
}
