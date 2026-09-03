const KIBIBYTE = 1024;
const MEBIBYTE = KIBIBYTE * KIBIBYTE;

export function formatFileSize(size: number) {
  const safeSize = Number.isFinite(size) && size > 0 ? size : 0;
  if (safeSize < KIBIBYTE) return `${Math.round(safeSize)} B`;
  if (safeSize < MEBIBYTE) return `${Math.max(1, Math.round(safeSize / KIBIBYTE))} KB`;
  return `${(safeSize / MEBIBYTE).toFixed(1)} MB`;
}

export function readFileAsDataUrl(file: File) {
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(typeof reader.result === 'string' ? reader.result : '');
    reader.onerror = () => reject(new Error(`无法读取 ${file.name}`));
    reader.readAsDataURL(file);
  });
}
