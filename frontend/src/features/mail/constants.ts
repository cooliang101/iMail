export const MESSAGE_MOVE_UNDO_MS = 4_500;

export function remainingMessageMoveUndoMs(deadlineAt: number, now = Date.now()) {
  return Math.max(0, deadlineAt - now);
}
