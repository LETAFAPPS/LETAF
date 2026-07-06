-- Formas de pagamento cadastradas — espelha desktop/061.
--
-- Regras aplicadas (AI_RULES.md §6, §7):
-- - BaseFields obrigatórios + soft delete + synced.
-- - 1 default por company garantido por índice único parcial.
-- - Sem dados sensíveis (CVV, número completo) — esta fase é só
--   catalogação visual. Tokenização do gateway real virá depois.

CREATE TABLE payment_methods (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    -- 'card' | 'pix'
    kind TEXT NOT NULL,
    -- "Visa de crédito" | "PIX automático" | "Mastercard"
    label TEXT NOT NULL DEFAULT '',
    -- Últimos 4 dígitos formatados "••••4242" (vazio para PIX).
    masked TEXT NOT NULL DEFAULT '',
    -- Validade "MM/AA" (vazio para PIX).
    expiry TEXT NOT NULL DEFAULT '',
    -- Forma de pagamento padrão da assinatura.
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    deleted_at TIMESTAMP WITHOUT TIME ZONE,
    synced BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX idx_payment_methods_company ON payment_methods(company_id);
CREATE UNIQUE INDEX idx_payment_methods_one_default
    ON payment_methods(company_id)
    WHERE is_default = TRUE AND deleted_at IS NULL;
