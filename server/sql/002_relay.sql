CREATE TABLE IF NOT EXISTS relay_messages (
  seq BIGSERIAL PRIMARY KEY,
  id UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
  sender TEXT NOT NULL CHECK (sender IN ('rigel', 'muse')),
  recipient TEXT NOT NULL CHECK (recipient IN ('rigel', 'muse')),
  kind TEXT NOT NULL,
  payload JSONB NOT NULL,
  idempotency_key TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  expires_at TIMESTAMPTZ NOT NULL DEFAULT now() + interval '7 days',
  acknowledged_at TIMESTAMPTZ,
  UNIQUE (sender, idempotency_key),
  CHECK (sender <> recipient)
);

CREATE INDEX IF NOT EXISTS relay_messages_recipient_seq_idx
  ON relay_messages (recipient, seq)
  WHERE acknowledged_at IS NULL;

CREATE INDEX IF NOT EXISTS relay_messages_expires_idx ON relay_messages (expires_at);
