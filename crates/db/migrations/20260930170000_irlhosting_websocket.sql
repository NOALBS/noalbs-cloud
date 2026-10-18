-- IRLHosting's WebSocket stats: the server pushes stats instead of being polled.
ALTER TYPE stream_server_kind ADD VALUE IF NOT EXISTS 'IRLHOSTING_WEBSOCKET';
