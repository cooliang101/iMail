import type { DesktopHttpResponse } from '../desktop/http';
import type { MailService } from './contracts';

export class HttpMailService implements MailService {
  readonly kind = 'http' as const;

  constructor(private readonly requester: (path: string, options?: RequestInit) => Promise<DesktopHttpResponse>) {}

  request(path: string, options?: RequestInit) {
    return this.requester(path, options);
  }
}
