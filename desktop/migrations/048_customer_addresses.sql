-- Espelha server/023_customer_addresses.sql (Fase 9 — agora sincronizado).
-- Endereço de entrega do cliente, compartilhado entre web e desktop:
-- ambos gravam na mesma tabela e sincronizam por last-write-wins.
CREATE TABLE customer_addresses (
    id            TEXT      PRIMARY KEY,
    company_id    TEXT      NOT NULL,
    customer_id   TEXT      NOT NULL,
    label         TEXT      NOT NULL,
    custom_label  TEXT      NULL,
    street        TEXT      NOT NULL,
    number        TEXT      NOT NULL,
    neighborhood  TEXT      NOT NULL,
    apartment     TEXT      NULL,
    created_at    TEXT      NOT NULL,
    updated_at    TEXT      NOT NULL,
    deleted_at    TEXT      NULL,
    synced        BOOLEAN   NOT NULL DEFAULT 0
);

CREATE INDEX idx_customer_addresses_tenant
    ON customer_addresses (company_id, customer_id)
    WHERE deleted_at IS NULL;

CREATE INDEX customer_addresses_updated_at_idx
    ON customer_addresses (company_id, updated_at);
