export type RuntimeKind = 'web' | 'tauri';

export type DownloadRequest = {
  url: string;
  filename: string;
};
export type SystemNotification = {
  title: string;
  body?: string;
  tag?: string;
  target?: NotificationTarget;
};

export type NotificationTarget = {
  messageId: string;
  accountEmail?: string;
};

export interface PlatformRuntime {
  kind: RuntimeKind;
  openExternal(url: string): Promise<void>;
  saveDownload(input: DownloadRequest): Promise<void>;
  prepareNotifications(): Promise<boolean>;
  notify(input: SystemNotification): Promise<void>;
  subscribeNotificationClicks(listener: (target: NotificationTarget) => void): () => void;
}
