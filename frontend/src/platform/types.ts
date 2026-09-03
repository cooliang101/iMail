export type RuntimeKind = 'web' | 'tauri';

export type DownloadRequest = {
  url: string;
  filename: string;
};
export type TextSaveRequest = {
  text: string;
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

export type ShareRequest = {
  title?: string;
  text?: string;
  url?: string;
};
export type ShareOutcome = 'shared' | 'copied';

export interface PlatformRuntime {
  kind: RuntimeKind;
  openExternal(url: string): Promise<void>;
  share?: (input: ShareRequest) => Promise<ShareOutcome>;
  saveDownload(input: DownloadRequest): Promise<void>;
  saveImage(input: DownloadRequest): Promise<void>;
  saveText(input: TextSaveRequest): Promise<void>;
  prepareNotifications(): Promise<boolean>;
  notify(input: SystemNotification): Promise<void>;
  subscribeNotificationClicks(listener: (target: NotificationTarget) => void): () => void;
}
