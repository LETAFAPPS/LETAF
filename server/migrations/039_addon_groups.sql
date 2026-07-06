-- Grupos de adicionais (Fase 4 — AI_RULES.md §6, §11).
--
-- Cada grupo concentra a regra de seleção (single|multi, min/max) e
-- agrega os itens em `addons`. Isolamento multi-tenant via company_id.
CREATE TABLE IF NOT EXISTS addon_groups (
    id          UUID PRIMARY KEY,
    company_id  UUID NOT NULL REFERENCES companies(id),
    name        TEXT NOT NULL,
    selection   TEXT NOT NULL,                -- "single" | "multi"
    min_select  INTEGER NOT NULL DEFAULT 0,
    max_select  INTEGER NOT NULL DEFAULT 0,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  TIMESTAMP NOT NULL DEFAULT now(),
    updated_at  TIMESTAMP NOT NULL DEFAULT now(),
    deleted_at  TIMESTAMP,
    synced      BOOLEAN NOT NULL DEFAULT false
);

CREATE INDEX IF NOT EXISTS idx_addon_groups_company_id ON addon_groups(company_id);
