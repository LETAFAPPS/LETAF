-- Itens individuais de adicional (Fase 4) — espelha 040_addons.sql do server.
CREATE TABLE IF NOT EXISTS addons (
    id          TEXT PRIMARY KEY,
    company_id  TEXT NOT NULL,
    group_id    TEXT NOT NULL,
    name        TEXT NOT NULL,
    price       REAL NOT NULL DEFAULT 0,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    active      INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT,
    synced      INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_addons_company_id ON addons(company_id);
CREATE INDEX IF NOT EXISTS idx_addons_group_id ON addons(company_id, group_id);
