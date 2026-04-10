export interface ClipboardAdapter {
  readText(): Promise<string | null>;
  writeText(text: string): Promise<void>;
}
