export const IMAGE_MIN_WIDTH_PX = 17;
export const IMAGE_DRAG_THRESHOLD_PX = 5;

export function imageDragWidth(
  startWidth: number,
  deltaX: number,
  maxWidth: number,
): number {
  return Math.min(
    maxWidth,
    Math.max(IMAGE_MIN_WIDTH_PX, Math.round(startWidth + deltaX)),
  );
}

export function imageDragActivated(deltaX: number, deltaY: number): boolean {
  return Math.abs(deltaX) >= IMAGE_DRAG_THRESHOLD_PX ||
    Math.abs(deltaY) >= IMAGE_DRAG_THRESHOLD_PX;
}

export function persistedImageWidth(width: number): number {
  return Math.round(width);
}
