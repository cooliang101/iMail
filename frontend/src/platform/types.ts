export type RuntimeKind = 'web' | 'tauri';

export type DownloadRequest = {
  url: string;
  filename: string;
};
export type DesktopNotification = {
  title: string;
  body?: string;
};

export interface PlatformRuntime {
  kind: RuntimeKind;
  openExternal(url: string): Promise<void>;
  saveDownload(input: DownloadRequest): Promise<void>;
  notify(input: DesktopNotification): Promise<void>;
}
