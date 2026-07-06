-- Lançamentos financeiros (contas a pagar / a receber) — Fase 11.
-- Suporta parcelamento e recorrência via parent_id.
-- Status 'overdue' NÃO é persistido: derivado em runtime
-- (status IN ('pending','scheduled') AND due_date < today).
CREATE TABLE finance_entries (
    id TEXT PRIMARY KEY,
    company_id TEXT NOT NULL,
    -- 'payable' | 'receivable'
    kind TEXT NOT NULL,
    description TEXT NOT NULL,
    party_id TEXT,
    party_name TEXT NOT NULL DEFAULT '',
    -- 'supplier' | 'customer' | 'other'
    party_type TEXT NOT NULL DEFAULT 'other',
    category_id TEXT,
    amount REAL NOT NULL,
    -- Datas armazenadas como ISO-8601 (YYYY-MM-DD) ordenáveis.
    due_date TEXT NOT NULL,
    paid_at TEXT,
    -- 'pending' | 'scheduled' | 'paid' | 'received' | 'cancelled'
    status TEXT NOT NULL DEFAULT 'pending',
    payment_method TEXT,
    notes TEXT,
    -- 'once' | 'weekly' | 'monthly' | 'custom'
    recurrence TEXT NOT NULL DEFAULT 'once',
    -- parent_id == id quando este é o cabeça do grupo (lançamento
    -- isolado ou cabeça de parcelamento/recorrência).
    parent_id TEXT NOT NULL,
    installment_index INTEGER NOT NULL DEFAULT 1,
    installment_total INTEGER NOT NULL DEFAULT 1,
    -- Futuro: vínculo com pedido (receivable gerado por venda PDV).
    order_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    synced INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_finance_entries_company ON finance_entries(company_id);
CREATE INDEX idx_finance_entries_company_kind ON finance_entries(company_id, kind);
CREATE INDEX idx_finance_entries_company_status ON finance_entries(company_id, status);
CREATE INDEX idx_finance_entries_company_due_date ON finance_entries(company_id, due_date);
CREATE INDEX idx_finance_entries_company_synced ON finance_entries(company_id, synced);
CREATE INDEX idx_finance_entries_parent ON finance_entries(parent_id);
