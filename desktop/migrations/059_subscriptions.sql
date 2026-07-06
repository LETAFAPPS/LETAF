-- Assinatura e faturas — espelha server/059.
--
-- Regras aplicadas (AI_RULES.md §6, §7):
-- - BaseFields obrigatórios; soft delete + synced (offline-first).
-- - Catálogo de planos hardcoded no SubscriptionService (super admin
--   no futuro substitui por tabela `plans` sincronizada).
-- - Seed inicial (subscription Mensal + 5 faturas históricas) feito
--   em runtime pelo SubscriptionService.ensure_seed na 1ª execução.

CREATE TABLE subscriptions (
    id TEXT PRIMARY KEY,
    company_id TEXT NOT NULL,
    -- 'monthly' | 'semestral' | 'annual'
    plan_kind TEXT NOT NULL DEFAULT 'monthly',
    -- YYYY-MM-DD; NULL = não houver cobrança agendada
    next_charge_date TEXT,
    -- 'active' | 'cancelled' | 'overdue'
    status TEXT NOT NULL DEFAULT 'active',
    payment_method_kind TEXT NOT NULL DEFAULT 'card',
    payment_method_label TEXT NOT NULL DEFAULT '',
    payment_method_expiry TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    synced INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_subscriptions_company ON subscriptions(company_id);

CREATE TABLE subscription_invoices (
    id TEXT PRIMARY KEY,
    company_id TEXT NOT NULL,
    subscription_id TEXT NOT NULL,
    number TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    amount REAL NOT NULL,
    method_kind TEXT NOT NULL DEFAULT 'card',
    method_label TEXT NOT NULL DEFAULT '',
    -- 'pending' | 'paid' | 'failed'
    status TEXT NOT NULL DEFAULT 'pending',
    issued_at TEXT NOT NULL,
    paid_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    synced INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_subscription_invoices_company ON subscription_invoices(company_id);
CREATE INDEX idx_subscription_invoices_company_issued ON subscription_invoices(company_id, issued_at);
