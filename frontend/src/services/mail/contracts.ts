import type { DesktopHttpResponse } from '../desktop/http';

export type MailServiceKind = 'http' | 'tauri-embedded';

export interface MailService {
  readonly kind: MailServiceKind;
  request(path: string, options?: RequestInit): Promise<DesktopHttpResponse>;
}
