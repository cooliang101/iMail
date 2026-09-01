export function dateTimeLocalValue(value: Date) {
  const year = value.getFullYear();
  const month = String(value.getMonth() + 1).padStart(2, '0');
  const day = String(value.getDate()).padStart(2, '0');
  const hour = String(value.getHours()).padStart(2, '0');
  const minute = String(value.getMinutes()).padStart(2, '0');
  return `${year}-${month}-${day}T${hour}:${minute}`;
}

export function nextHourLocalValue(now = new Date()) {
  const value = new Date(now.getTime() + 60 * 60 * 1000);
  value.setSeconds(0, 0);
  return dateTimeLocalValue(value);
}
