-- Tabela de horário de funcionamento (§6: entidade completa com company_id e synced).
-- UNIQUE(company_id, day_of_week): apenas um registro por dia por empresa.
CREATE TABLE IF NOT EXISTS business_hours (
    id             UUID PRIMARY KEY,
    company_id     UUID NOT NULL REFERENCES companies(id),
    day_of_week    INTEGER NOT NULL,
    open_time      TEXT NOT NULL DEFAULT '08:00',
    close_time     TEXT NOT NULL DEFAULT '18:00',
    is_open        BOOLEAN NOT NULL DEFAULT FALSE,
    created_at     TIMESTAMPTZ NOT NULL,
    updated_at     TIMESTAMPTZ NOT NULL,
    deleted_at     TIMESTAMPTZ,
    synced         BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE(company_id, day_of_week)
);

CREATE INDEX IF NOT EXISTS idx_business_hours_company ON business_hours(company_id);
