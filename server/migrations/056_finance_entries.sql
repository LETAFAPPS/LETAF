-- Lançamentos financeiros — espelha desktop/056.
CREATE TABLE finance_entries (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    kind TEXT NOT NULL,
    description TEXT NOT NULL,
    party_id UUID,
    party_name TEXT NOT NULL DEFAULT '',
    party_type TEXT NOT NULL DEFAULT 'other',
    category_id UUID,
    amount DOUBLE PRECISION NOT NULL,
    due_date DATE NOT NULL,
    paid_at TIMESTAMP WITHOUT TIME ZONE,
    status TEXT NOT NULL DEFAULT 'pending',
    payment_method TEXT,
    notes TEXT,
    recurrence TEXT NOT NULL DEFAULT 'once',
    parent_id UUID NOT NULL,
    installment_index INTEGER NOT NULL DEFAULT 1,
    installment_total INTEGER NOT NULL DEFAULT 1,
    order_id UUID,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    deleted_at TIMESTAMP WITHOUT TIME ZONE,
    synced BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX idx_finance_entries_company ON finance_entries(company_id);
CREATE INDEX idx_finance_entries_company_kind ON finance_entries(company_id, kind);
CREATE INDEX idx_finance_entries_company_status ON finance_entries(company_id, status);
CREATE INDEX idx_finance_entries_company_due_date ON finance_entries(company_id, due_date);
CREATE INDEX idx_finance_entries_parent ON finance_entries(parent_id);
