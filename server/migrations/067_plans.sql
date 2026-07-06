-- Catálogo de planos gerido pelo super admin (nível PLATAFORMA, sem
-- company_id — é global/cross-tenant, como o super admin). As lojas
-- apenas leem os planos ativos. A assinatura (billing) fará snapshot dos
-- termos na Fase 2.
CREATE TABLE IF NOT EXISTS plans (
    id              UUID PRIMARY KEY,
    name            TEXT NOT NULL,
    amount          DOUBLE PRECISION NOT NULL,        -- valor cobrado por ciclo (R$)
    period_months   INTEGER NOT NULL,                 -- meses por cobrança (1, 6, 12, ...)
    trial_days      INTEGER NOT NULL DEFAULT 0,       -- período gratuito antes da 1ª cobrança
    description     TEXT NOT NULL DEFAULT '',
    highlight_label TEXT NOT NULL DEFAULT '',          -- selo (ex.: "MELHOR VALOR")
    active          BOOLEAN NOT NULL DEFAULT TRUE,
    sort_order      INTEGER NOT NULL DEFAULT 0,
    created_at      TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMP NOT NULL DEFAULT NOW(),
    deleted_at      TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_plans_active
    ON plans (sort_order) WHERE deleted_at IS NULL AND active = TRUE;
