-- Assinatura e faturas — espelha desktop/059.
--
-- Regras aplicadas (AI_RULES.md §6, §7):
-- - BaseFields obrigatórios em ambas tabelas.
-- - Catálogo de planos vive no service como constantes por enquanto
--   (super admin no futuro substitui por uma tabela `plans` sincronizada).

CREATE TABLE subscriptions (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    plan_kind TEXT NOT NULL DEFAULT 'monthly',
    next_charge_date DATE,
    status TEXT NOT NULL DEFAULT 'active',
    payment_method_kind TEXT NOT NULL DEFAULT 'card',
    payment_method_label TEXT NOT NULL DEFAULT '',
    payment_method_expiry TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    deleted_at TIMESTAMP WITHOUT TIME ZONE,
    synced BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX idx_subscriptions_company ON subscriptions(company_id);

CREATE TABLE subscription_invoices (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    subscription_id UUID NOT NULL,
    number TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    amount NUMERIC(14, 2) NOT NULL,
    method_kind TEXT NOT NULL DEFAULT 'card',
    method_label TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    issued_at DATE NOT NULL,
    paid_at TIMESTAMP WITHOUT TIME ZONE,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    deleted_at TIMESTAMP WITHOUT TIME ZONE,
    synced BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX idx_subscription_invoices_company ON subscription_invoices(company_id);
CREATE INDEX idx_subscription_invoices_company_issued ON subscription_invoices(company_id, issued_at);
