-- Cobranças avulsas (gateway de pagamento) — espelha desktop/060.
--
-- Regras aplicadas (AI_RULES.md §6, §7):
-- - BaseFields obrigatórios + soft delete + synced.
-- - `txid` é a chave externa do gateway (PIX TXID, IDs do Pagar.me etc).
-- - Por enquanto só PIX/Efi; coluna `gateway` permite trocar/adicionar
--   sem migração nova.
-- - `invoice_id` opcional: a cobrança pode ser de "pagar fatura agora"
--   (vinculada a uma `subscription_invoices`) ou avulsa.
CREATE TABLE payment_charges (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    invoice_id UUID,
    gateway TEXT NOT NULL DEFAULT 'efi',
    method TEXT NOT NULL DEFAULT 'pix',
    -- TXID retornado pelo gateway (PIX: 26 chars hex). NULL antes
    -- de criar a cobrança remota (rascunho).
    txid TEXT,
    amount NUMERIC(14, 2) NOT NULL,
    -- 'pending' | 'paid' | 'expired' | 'failed' | 'cancelled'
    status TEXT NOT NULL DEFAULT 'pending',
    -- Conteúdo do BR Code (string copiável). NULL antes de criar.
    pix_copia_cola TEXT,
    -- PNG do QR Code em base64. Salvar facilita exibição offline na UI.
    qr_code_b64 TEXT,
    -- ISO-8601 do `loc.expiracao` calculado pelo gateway.
    expires_at TIMESTAMP WITHOUT TIME ZONE,
    paid_at TIMESTAMP WITHOUT TIME ZONE,
    -- Mensagem de erro do gateway (para diagnóstico) — não exibida ao usuário.
    last_error TEXT,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    deleted_at TIMESTAMP WITHOUT TIME ZONE,
    synced BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX idx_payment_charges_company ON payment_charges(company_id);
CREATE INDEX idx_payment_charges_company_status ON payment_charges(company_id, status);
CREATE INDEX idx_payment_charges_invoice ON payment_charges(invoice_id);
CREATE UNIQUE INDEX idx_payment_charges_txid
    ON payment_charges(gateway, txid)
    WHERE txid IS NOT NULL;
