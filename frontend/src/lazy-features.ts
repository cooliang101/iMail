import { createElement, forwardRef, type ComponentPropsWithoutRef, type ElementType } from 'react';

function createPreloadableComponent<T extends ElementType>(loader: () => Promise<{ default: T }>) {
  let loaded: T | undefined;
  let failure: unknown;
  let pending: Promise<void> | undefined;

  const load = () => {
    if (loaded) return Promise.resolve();
    if (failure) return Promise.reject(failure);
    pending ??= loader().then((module) => { loaded = module.default; }).catch((error: unknown) => {
      failure = error;
      throw error;
    });
    return pending;
  };
  const preload = () => load().catch((error: unknown) => {
    if (!loaded) {
      failure = undefined;
      pending = undefined;
    }
    throw error;
  });

  const Preloadable = forwardRef<unknown, ComponentPropsWithoutRef<T>>((props, ref) => {
    if (failure) throw failure;
    if (!loaded) throw load();
    return createElement(loaded, ref == null ? props : { ...props, ref });
  });

  return { Component: Preloadable as unknown as T, preload };
}

const composePane = createPreloadableComponent(() => import('./features/compose/ComposePane').then((module) => ({ default: module.ComposePane })));
const settingsModal = createPreloadableComponent(() => import('./features/settings/SettingsModal').then((module) => ({ default: module.SettingsModal })));
const addAccountModal = createPreloadableComponent(() => import('./features/accounts/AddAccountModal').then((module) => ({ default: module.AddAccountModal })));
const createApiTokenModal = createPreloadableComponent(() => import('./features/developer/CreateApiTokenModal').then((module) => ({ default: module.CreateApiTokenModal })));
const createMcpTokenModal = createPreloadableComponent(() => import('./features/developer/CreateMcpTokenModal').then((module) => ({ default: module.CreateMcpTokenModal })));
const notificationsModal = createPreloadableComponent(() => import('./features/organize/NotificationsModal').then((module) => ({ default: module.NotificationsModal })));
const labelModal = createPreloadableComponent(() => import('./features/organize/LabelModal').then((module) => ({ default: module.LabelModal })));
const snoozeModal = createPreloadableComponent(() => import('./features/organize/SnoozeModal').then((module) => ({ default: module.SnoozeModal })));
const workspaceModal = createPreloadableComponent(() => import('./features/organize/WorkspaceModal').then((module) => ({ default: module.WorkspaceModal })));
const preferencesSyncErrorDialog = createPreloadableComponent(() => import('./features/settings/PreferencesSyncErrorDialog').then((module) => ({ default: module.PreferencesSyncErrorDialog })));
const appContextMenu = createPreloadableComponent(() => import('./features/context-menu/AppContextMenu').then((module) => ({ default: module.AppContextMenu })));

export const ComposePane = composePane.Component;
export const SettingsModal = settingsModal.Component;
export const AddAccountModal = addAccountModal.Component;
export const CreateApiTokenModal = createApiTokenModal.Component;
export const CreateMcpTokenModal = createMcpTokenModal.Component;
export const NotificationsModal = notificationsModal.Component;
export const LabelModal = labelModal.Component;
export const SnoozeModal = snoozeModal.Component;
export const WorkspaceModal = workspaceModal.Component;
export const PreferencesSyncErrorDialog = preferencesSyncErrorDialog.Component;
export const AppContextMenu = appContextMenu.Component;

const deferredFeatureLoaders = [
  composePane.preload,
  settingsModal.preload,
  addAccountModal.preload,
  createApiTokenModal.preload,
  createMcpTokenModal.preload,
  notificationsModal.preload,
  labelModal.preload,
  snoozeModal.preload,
  workspaceModal.preload,
  appContextMenu.preload,
  preferencesSyncErrorDialog.preload,
];

export function preloadDeferredFeaturesDuringIdle() {
  let cancelled = false;
  let nextIndex = 0;
  let idleHandle: number | undefined;
  let timerHandle: number | undefined;

  const scheduleNext = () => {
    if (cancelled || nextIndex >= deferredFeatureLoaders.length) return;
    const preloadNext = () => {
      if (cancelled) return;
      const loader = deferredFeatureLoaders[nextIndex++];
      void loader().catch(() => undefined).finally(scheduleNext);
    };
    if (typeof window.requestIdleCallback === 'function') {
      idleHandle = window.requestIdleCallback(preloadNext, { timeout: 3_000 });
    } else {
      timerHandle = window.setTimeout(preloadNext, nextIndex === 0 ? 1_200 : 250);
    }
  };

  scheduleNext();
  return () => {
    cancelled = true;
    if (idleHandle !== undefined && typeof window.cancelIdleCallback === 'function') window.cancelIdleCallback(idleHandle);
    if (timerHandle !== undefined) window.clearTimeout(timerHandle);
  };
}
