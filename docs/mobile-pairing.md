# Recall Mobile Pairing

Recall desktop exposes a local-only receive endpoint so a phone can push memories to the desktop on the same Wi-Fi network.

## Pairing payload

Open `Settings > Pairing` in the desktop app. The QR payload is JSON:

```json
{
  "protocol": "recall-local-pairing",
  "version": 1,
  "deviceId": "desktop-...",
  "desktopName": "Recall Desktop",
  "endpoint": "http://192.168.1.20:47653",
  "secret": "rcp_..."
}
```

The phone should store `endpoint` and `secret` locally. Resetting pairing rotates the secret and invalidates older phone pairings.

## Ping

```http
GET /api/ping
Authorization: Bearer <secret>
```

Success:

```json
{
  "ok": true,
  "deviceId": "desktop-...",
  "desktopName": "Recall Desktop",
  "endpoint": "http://192.168.1.20:47653"
}
```

## Push memory

```http
POST /api/push-memory
Authorization: Bearer <secret>
Content-Type: application/json
```

Body:

```json
{
  "memory": {
    "id": "phone-generated-id",
    "title": "Optional title",
    "content": "Text captured on phone",
    "url": "https://example.com",
    "source": "iPhone",
    "note": "Optional note",
    "memoryType": "article",
    "createdAt": 1776000000000,
    "imageUri": "Optional local mobile URI",
    "previewText": "Optional preview"
  }
}
```

Rules:

- `Authorization` is required. Invalid secrets return `401`.
- `memory.id` and `memory.createdAt` are required.
- At least one of `content`, `url`, or `previewText` is required.
- `createdAt` can be seconds or milliseconds since Unix epoch.
- Duplicate mobile sends with the same `memory.id` are accepted but not inserted twice.
- Received memories are stored locally, emitted to the desktop UI immediately, and scheduled for normal link enrichment.

## Same Wi-Fi testing

1. Connect phone and desktop to the same Wi-Fi network.
2. Open `Settings > Pairing` on desktop.
3. Scan or copy the QR payload into the mobile app.
4. From the phone, call `GET <endpoint>/api/ping` with the bearer secret.
5. Send a test payload to `POST <endpoint>/api/push-memory`.
6. The memory should appear in Recall Home/Library/Search without restarting the app.
