// Follow the newest day until the user scrubs back. Keep that day selected
// when a refresh inserts earlier photographs or replaces a day's chosen face.
export function selectedFrameIndex(
  frames: readonly { capture_day: string }[],
  selectedDay?: string,
) {
  const index = selectedDay
    ? frames.findIndex((frame) => frame.capture_day === selectedDay)
    : -1;
  return index < 0 ? frames.length - 1 : index;
}
