CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    peer_id TEXT NOT NULL,
    direction TEXT NOT NULL CHECK (direction IN ('incoming', 'outgoing')),
    body TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'sent', 'delivered', 'unknown', 'failed')),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    delivered_at DATETIME,
    UNIQUE (peer_id, message_id, direction)
);

CREATE INDEX IF NOT EXISTS idx_messages_peer_created_at
    ON messages (peer_id, created_at, id);

CREATE INDEX IF NOT EXISTS idx_messages_session
    ON messages (session_id, peer_id, id);
