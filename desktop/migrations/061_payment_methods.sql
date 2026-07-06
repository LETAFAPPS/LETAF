-- Formas de pagamento cadastradas — espelha server/061.
--
-- Regras aplicadas (AI_RULES.md §6, §7):
-- - BaseFields obrigatórios + soft delete + synced.
-- - 1 default por company.

CREATE TABLE payment_methods (
    id TEXT PRIMARY KEY,
    company_id TEXT NOT NULL,
    -- 'card' | 'pix'
    kind TEXT NOT NULL,
    label TEXT NOT NULL DEFAULT '',
    masked TEXT NOT NULL DEFAULT '',
    expiry TEXT NOT NULL DEFAULT '',
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    synced INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_payment_methods_company ON payment_methods(company_id);
CREATE UNIQUE INDEX idx_payment_methods_one_default
    ON payment_methods(company_id)
    WHERE is_default = 1 AND deleted_at IS NULL;
