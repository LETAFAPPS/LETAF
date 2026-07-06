-- Banner promocional do cardápio web (Fase 7).
-- `item_type` valida na app via allowlist em letaf-core::banner::ITEM_TYPES.
-- `item_id` aponta para products (FK lógico, sem CASCADE — banner pode
-- existir antes do produto vir do sync ou ficar órfão se o produto for
-- deletado; o backend filtra produtos inválidos antes de renderizar).

CREATE TABLE banners (
    id          UUID        PRIMARY KEY,
    company_id  UUID        NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    title       TEXT        NOT NULL,
    image_data  TEXT        NOT NULL,
    item_type   TEXT        NOT NULL,
    item_id     UUID        NULL,
    item_url    TEXT        NULL,
    active      BOOLEAN     NOT NULL DEFAULT TRUE,
    sort_order  INTEGER     NOT NULL DEFAULT 0,
    created_at  TIMESTAMP   NOT NULL,
    updated_at  TIMESTAMP   NOT NULL,
    deleted_at  TIMESTAMP   NULL,
    synced      BOOLEAN     NOT NULL DEFAULT FALSE
);

CREATE INDEX banners_company_active_idx
    ON banners (company_id, active)
    WHERE deleted_at IS NULL;

CREATE INDEX banners_updated_at_idx
    ON banners (company_id, updated_at);
