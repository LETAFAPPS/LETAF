-- Espelha server/047_coupons.sql (Fase 8).
-- SQLite usa TEXT para UUID/timestamps e BOOLEAN ↔ INTEGER (0/1).

CREATE TABLE coupons (
    id              TEXT      PRIMARY KEY,
    company_id      TEXT      NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    title           TEXT      NOT NULL,
    code            TEXT      NOT NULL,
    coupon_type     TEXT      NOT NULL,
    discount_kind   TEXT      NOT NULL,
    discount_value  REAL      NOT NULL DEFAULT 0,
    min_order_value REAL      NOT NULL DEFAULT 0,
    max_discount    REAL      NOT NULL DEFAULT 0,
    per_user_limit  INTEGER   NOT NULL DEFAULT 0,
    usage_limit     INTEGER   NOT NULL DEFAULT 0,
    valid_from      TEXT      NULL,
    valid_until     TEXT      NULL,
    active          BOOLEAN   NOT NULL DEFAULT 1,
    created_at      TEXT      NOT NULL,
    updated_at      TEXT      NOT NULL,
    deleted_at      TEXT      NULL,
    synced          BOOLEAN   NOT NULL DEFAULT 0
);

CREATE UNIQUE INDEX coupons_company_code_uidx
    ON coupons (company_id, code)
    WHERE deleted_at IS NULL;

CREATE INDEX coupons_company_active_idx
    ON coupons (company_id, active)
    WHERE deleted_at IS NULL;

CREATE INDEX coupons_updated_at_idx
    ON coupons (company_id, updated_at);
