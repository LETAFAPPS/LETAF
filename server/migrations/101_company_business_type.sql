-- Associa cada empresa a um TIPO de estabelecimento (ramo) do catálogo
-- `business_types`. Nullable: empresa pode não ter tipo definido. Atribuído
-- SOMENTE pelo super admin (painel) — server-authoritative, como `active`.
ALTER TABLE companies
    ADD COLUMN IF NOT EXISTS business_type_id UUID REFERENCES business_types (id);

CREATE INDEX IF NOT EXISTS idx_companies_business_type
    ON companies (business_type_id) WHERE business_type_id IS NOT NULL;
