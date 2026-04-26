export const isBlank = (value: string | null | undefined) =>
  !value || value.trim().length === 0;

export const toNullable = (value: string) => {
  const trimmed = value.trim();
  return trimmed.length === 0 ? null : trimmed;
};
