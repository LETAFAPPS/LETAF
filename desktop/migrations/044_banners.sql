-- Espelha server/046_banners.sql (Fase 7).
-- SQLite usa TEXT para UUID e BOOLEAN ↔ INTEGER (0/1).

CREATE TABLE banners (
    id          TEXT      PRIMARY KEY,
    company_id  TEXT      NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    title       TEXT      NOT NULL,
    image_data  TEXT      NOT NULL,
    item_type   TEXT      NOT NULL,
    item_id     TEXT      NULL,
    item_url    TEXT      NULL,
    active      BOOLEAN   NOT NULL DEFAULT 1,
    sort_order  INTEGER   NOT NULL DEFAULT 0,
    created_at  TEXT      NOT NULL,
    updated_at  TEXT      NOT NULL,
    deleted_at  TEXT      NULL,
    synced      BOOLEAN   NOT NULL DEFAULT 0
);

CREATE INDEX banners_company_active_idx
    ON banners (company_id, active)
    WHERE deleted_at IS NULL;

CREATE INDEX banners_updated_at_idx
    ON banners (company_id, updated_at);
