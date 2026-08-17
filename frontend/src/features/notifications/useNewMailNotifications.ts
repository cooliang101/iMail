import { useEffect, useRef } from 'preact/compat';
import { usePlatform } from '../../platform/runtime';
import { desktopLog, describeDesktopLogValue, subscribeSyncEvents } from '../../services';
import { newMailNotificationFromEvent } from './new-mail-notifications';

const MAX_REMEMBERED_EVENTS = 200;

export function useNewMailNotifications(enabled: boolean, onOpenMessage: (messageId: string, accountEmail?: string) => void) {
  const platform = usePlatform();
  const seenEvents = useRef(new Set<string>());
  const onOpenMessageRef = useRef(onOpenMessage);

  useEffect(() => { onOpenMessageRef.current = onOpenMessage; }, [onOpenMessage]);
  useEffect(() => platform.subscribeNotificationClicks((target) => {
    onOpenMessageRef.current(target.messageId, target.accountEmail);
  }), [platform]);

  useEffect(() => {
    if (!enabled) return;
    const prepare = () => {
      void platform.prepareNotifications()
        .then((granted) => desktopLog(granted ? 'info' : 'warn', 'notification.permission', granted ? 'granted' : 'denied'))
        .catch((error) => desktopLog('error', 'notification.permission_failed', describeDesktopLogValue(error)));
    };
    const prepareWebNotifications = () => {
      window.removeEventListener('pointerdown', prepareWebNotifications);
      window.removeEventListener('keydown', prepareWebNotifications);
      prepare();
    };
    if (platform.kind === 'tauri') prepare();
    else {
      window.addEventListener('pointerdown', prepareWebNotifications, { once: true });
      window.addEventListener('keydown', prepareWebNotifications, { once: true });
    }
    const unsubscribe = subscribeSyncEvents(['message.created'], (event) => {
      const notification = newMailNotificationFromEvent(event.data);
      if (!notification || seenEvents.current.has(notification.eventKey)) return;
      seenEvents.current.add(notification.eventKey);
      if (seenEvents.current.size > MAX_REMEMBERED_EVENTS) {
        const oldest = seenEvents.current.values().next().value;
        if (oldest) seenEvents.current.delete(oldest);
      }
      void desktopLog('info', 'notification.dispatch', `event=${notification.eventKey}`)
        .then(() => platform.notify(notification))
        .then(() => desktopLog('info', 'notification.dispatched', `event=${notification.eventKey}`))
        .catch((error) => desktopLog('error', 'notification.failed', describeDesktopLogValue(error)));
    });
    return () => {
      window.removeEventListener('pointerdown', prepareWebNotifications);
      window.removeEventListener('keydown', prepareWebNotifications);
      unsubscribe();
    };
  }, [enabled, platform]);
}
