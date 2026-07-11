-- Cupons de desconto (Fase 8).
-- `coupon_type` e `discount_kind` validados na app via allowlists em
-- letaf-core::coupon (COUPON_TYPES / DISCOUNT_KINDS).
-- `code` é único por empresa (índice parcial ignora soft-deleted).

CREATE TABLE coupons (
    id              UUID        PRIMARY KEY,
    company_id      UUID        NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    title           TEXT        NOT NULL,
    code            TEXT        NOT NULL,
    coupon_type     TEXT        NOT NULL,
    discount_kind   TEXT        NOT NULL,
    discount_value  DOUBLE PRECISION NOT NULL DEFAULT 0,
    min_order_value DOUBLE PRECISION NOT NULL DEFAULT 0,
    max_discount    DOUBLE PRECISION NOT NULL DEFAULT 0,
    per_user_limit  INTEGER     NOT NULL DEFAULT 0,
    usage_limit     INTEGER     NOT NULL DEFAULT 0,
    valid_from      TIMESTAMP   NULL,
    valid_until     TIMESTAMP   NULL,
    active          BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMP   NOT NULL,
    updated_at      TIMESTAMP   NOT NULL,
    deleted_at      TIMESTAMP   NULL,
    synced          BOOLEAN     NOT NULL DEFAULT FALSE
);

-- Código único por empresa (apenas entre cupons não deletados).
CREATE UNIQUE INDEX coupons_company_code_uidx
    ON coupons (company_id, code)
    WHERE deleted_at IS NULL;

CREATE INDEX coupons_company_active_idx
    ON coupons (company_id, active)
    WHERE deleted_at IS NULL;

CREATE INDEX coupons_updated_at_idx
    ON coupons (company_id, updated_at);
