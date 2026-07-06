-- Tabela de horário de funcionamento (§6: entidade completa com company_id e synced).
-- UNIQUE(company_id, day_of_week): apenas um registro por dia por empresa.
CREATE TABLE IF NOT EXISTS business_hours (
    id             TEXT PRIMARY KEY,
    company_id     TEXT NOT NULL,
    day_of_week    INTEGER NOT NULL,
    open_time      TEXT NOT NULL DEFAULT '08:00',
    close_time     TEXT NOT NULL DEFAULT '18:00',
    is_open        INTEGER NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    deleted_at     TEXT,
    synced         INTEGER NOT NULL DEFAULT 0,
    UNIQUE(company_id, day_of_week)
);

CREATE INDEX IF NOT EXISTS idx_business_hours_company ON business_hours(company_id);
