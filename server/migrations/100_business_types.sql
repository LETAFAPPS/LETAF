-- Catálogo de TIPOS DE EMPRESA (ramo do estabelecimento), gerido pelo super
-- admin (nível PLATAFORMA, sem company_id — global/cross-tenant, como os
-- planos). Ex.: "Restaurante", "Loja". Cada empresa terá um tipo (associação
-- em fase futura); tema do site e diferenças de produto virão depois.
CREATE TABLE IF NOT EXISTS business_types (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    active      BOOLEAN NOT NULL DEFAULT TRUE,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMP NOT NULL DEFAULT NOW(),
    deleted_at  TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_business_types_active
    ON business_types (sort_order) WHERE deleted_at IS NULL AND active = TRUE;
