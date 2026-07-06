-- Cobranças avulsas (gateway) — espelha server/060.
--
-- Regras aplicadas (AI_RULES.md §6, §7):
-- - BaseFields obrigatórios + soft delete + synced.
-- - `txid` é a chave externa do gateway (PIX TXID, etc).
-- - `invoice_id` opcional: vinculada a uma `subscription_invoices`
--   ou avulsa.
CREATE TABLE payment_charges (
    id TEXT PRIMARY KEY,
    company_id TEXT NOT NULL,
    invoice_id TEXT,
    -- 'efi' por enquanto; permite trocar/adicionar sem migração.
    gateway TEXT NOT NULL DEFAULT 'efi',
    -- 'pix' inicialmente; 'card' quando entrar tokenização.
    method TEXT NOT NULL DEFAULT 'pix',
    txid TEXT,
    amount REAL NOT NULL,
    -- 'pending' | 'paid' | 'expired' | 'failed' | 'cancelled'
    status TEXT NOT NULL DEFAULT 'pending',
    pix_copia_cola TEXT,
    qr_code_b64 TEXT,
    expires_at TEXT,
    paid_at TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    synced INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_payment_charges_company ON payment_charges(company_id);
CREATE INDEX idx_payment_charges_company_status ON payment_charges(company_id, status);
CREATE INDEX idx_payment_charges_invoice ON payment_charges(invoice_id);
CREATE UNIQUE INDEX idx_payment_charges_txid
    ON payment_charges(gateway, txid)
    WHERE txid IS NOT NULL;
