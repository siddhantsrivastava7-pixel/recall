# Bookmark Integration

Recall imports bookmarks locally from Chromium-based browser bookmark files and stores them as first-class memories.

## V1 Browsers

- Chrome
- Edge
- Brave

Safari is intentionally left for a later platform-specific implementation.

## Native Architecture

- `BookmarkIngestionService` lives in `src-tauri/src/services/bookmark_service.rs`
- browser file resolution is isolated behind `BrowserPathResolver`
- Windows paths are implemented in `src-tauri/src/platform/windows/mod.rs`
- macOS paths are implemented in `src-tauri/src/platform/mac/mod.rs`

Shared UI code does not hardcode browser bookmark paths.

## Storage Model

Bookmarks are stored in the existing `memories` table with additional metadata:

- `source_type = "bookmark"`
- `url`
- `external_id`
- `folder_path`
- `source_app = "chrome" | "edge" | "brave"`

Duplicate imports are prevented with a unique index on `(source_app, external_id)` when both values are present.

## Sync Model

- manual import can be triggered per browser from Settings
- auto-sync re-scans selected browsers on an interval from app settings
- sync only imports new bookmarks
- removed browser bookmarks are not deleted from Recall in V1

## Platform Notes

Windows bookmark files:

- Chrome: `%LOCALAPPDATA%\Google\Chrome\User Data\Default\Bookmarks`
- Edge: `%LOCALAPPDATA%\Microsoft\Edge\User Data\Default\Bookmarks`
- Brave: `%LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data\Default\Bookmarks`

macOS bookmark files:

- Chrome: `~/Library/Application Support/Google/Chrome/Default/Bookmarks`
- Edge: `~/Library/Application Support/Microsoft Edge/Default/Bookmarks`
- Brave: `~/Library/Application Support/BraveSoftware/Brave-Browser/Default/Bookmarks`

Future macOS follow-up areas:

- Safari bookmark storage and permissions
- profile-aware browser path resolution beyond the `Default` profile
- optional per-browser file watching instead of interval-based polling
