-- Itens individuais de adicional, atrelados a um grupo (Fase 4).
--
-- Preço é acréscimo em R$ ao preço base do produto (0.0 é válido —
-- ex.: "Sem cebola" gratuito). `active` permite ocultar do cardápio
-- web sem soft-delete.
CREATE TABLE IF NOT EXISTS addons (
    id          UUID PRIMARY KEY,
    company_id  UUID NOT NULL REFERENCES companies(id),
    group_id    UUID NOT NULL REFERENCES addon_groups(id),
    name        TEXT NOT NULL,
    price NUMERIC(14, 2) NOT NULL DEFAULT 0,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    active      BOOLEAN NOT NULL DEFAULT true,
    created_at  TIMESTAMP NOT NULL DEFAULT now(),
    updated_at  TIMESTAMP NOT NULL DEFAULT now(),
    deleted_at  TIMESTAMP,
    synced      BOOLEAN NOT NULL DEFAULT false
);

CREATE INDEX IF NOT EXISTS idx_addons_company_id ON addons(company_id);
CREATE INDEX IF NOT EXISTS idx_addons_group_id ON addons(company_id, group_id);
