export interface FileSystemAdapter {
  exportData(): Promise<string>;
  importData(): Promise<string>;
}
