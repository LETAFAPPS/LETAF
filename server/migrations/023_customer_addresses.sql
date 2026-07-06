-- Tabela de endereços de entrega dos clientes.
--
-- AI_RULES.md §6: BaseFields obrigatórios (UUID pk, company_id, timestamps, synced, soft-delete).
-- AI_RULES.md § isolamento: todas as queries filtram por company_id.

CREATE TABLE IF NOT EXISTS customer_addresses (
    id            UUID        NOT NULL PRIMARY KEY,
    company_id    UUID        NOT NULL,
    customer_id   UUID        NOT NULL,
    label         TEXT        NOT NULL,
    custom_label  TEXT,
    street        TEXT        NOT NULL,
    number        TEXT        NOT NULL,
    neighborhood  TEXT        NOT NULL,
    apartment     TEXT,
    created_at    TIMESTAMP   NOT NULL,
    updated_at    TIMESTAMP   NOT NULL,
    deleted_at    TIMESTAMP,
    synced        BOOLEAN     NOT NULL DEFAULT FALSE
);

CREATE INDEX IF NOT EXISTS idx_customer_addresses_tenant
    ON customer_addresses (company_id, customer_id)
    WHERE deleted_at IS NULL;
